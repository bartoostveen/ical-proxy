use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Optional Redis cache. Every operation fails softly: if Redis is not
/// configured or unavailable, the proxy simply works without caching.
#[derive(Clone)]
pub struct Cache {
    client: Option<redis::Client>,
    connection: Arc<Mutex<Option<ConnectionManager>>>,
}

impl Cache {
    /// Creates a cache for the given Redis URL. Caching is disabled when no
    /// URL is configured; the legacy `host:port` endpoint form is accepted
    /// too.
    pub fn new(endpoint: Option<&str>) -> Self {
        let endpoint = endpoint
            .map(str::trim)
            .filter(|endpoint| !endpoint.is_empty());

        let Some(endpoint) = endpoint else {
            return Self {
                client: None,
                connection: Arc::new(Mutex::new(None)),
            };
        };

        let url = if endpoint.contains("://") {
            endpoint.to_owned()
        } else {
            format!("redis://{endpoint}")
        };

        let client = match redis::Client::open(url) {
            Ok(client) => Some(client),
            Err(error) => {
                tracing::warn!("failed to configure Redis client: {error}");
                None
            }
        };

        Self {
            client,
            connection: Arc::new(Mutex::new(None)),
        }
    }

    /// Whether a Redis URL was configured.
    pub fn enabled(&self) -> bool {
        self.client.is_some()
    }

    async fn connection(&self) -> Option<ConnectionManager> {
        let client = self.client.as_ref()?;

        let mut connection = self.connection.lock().await;
        if let Some(connection) = connection.as_ref() {
            return Some(connection.clone());
        }

        let config = ConnectionManagerConfig::new()
            .set_number_of_retries(0)
            .set_connection_timeout(Some(Duration::from_secs(1)))
            .set_response_timeout(Some(Duration::from_secs(2)));

        match ConnectionManager::new_with_config(client.clone(), config).await {
            Ok(manager) => {
                *connection = Some(manager.clone());
                Some(manager)
            }
            Err(error) => {
                tracing::warn!("failed to connect to Redis: {error}");
                None
            }
        }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let mut connection = self.connection().await?;

        match redis::cmd("GET")
            .arg(key)
            .query_async::<Option<String>>(&mut connection)
            .await
        {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!("failed to read {key:?} from Redis: {error}");
                None
            }
        }
    }

    /// Stores a value that expires after `ttl` seconds.
    pub async fn set_ex(&self, key: &str, value: &str, ttl: i64) {
        let Some(mut connection) = self.connection().await else {
            return;
        };

        let result = redis::cmd("SET")
            .arg(key)
            .arg(value)
            .arg("EX")
            .arg(ttl)
            .query_async::<()>(&mut connection)
            .await;

        if let Err(error) = result {
            tracing::warn!("failed to write {key:?} to Redis: {error}");
        }
    }

    /// Stores a value that expires at an absolute Unix timestamp (in seconds).
    pub async fn set_ex_at(&self, key: &str, value: &str, timestamp: i64) {
        let Some(mut connection) = self.connection().await else {
            return;
        };

        let result = redis::cmd("SET")
            .arg(key)
            .arg(value)
            .arg("EXAT")
            .arg(timestamp)
            .query_async::<()>(&mut connection)
            .await;

        if let Err(error) = result {
            tracing::warn!("failed to write {key:?} to Redis: {error}");
        }
    }

    /// Returns the absolute Unix expiration timestamp of a key, if it has one.
    pub async fn expire_time(&self, key: &str) -> Option<i64> {
        let mut connection = self.connection().await?;

        match redis::cmd("EXPIRETIME")
            .arg(key)
            .query_async::<i64>(&mut connection)
            .await
        {
            Ok(timestamp) if timestamp > 0 => Some(timestamp),
            Ok(_) => None,
            Err(error) => {
                tracing::warn!("failed to get expiration time of {key:?}: {error}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_blank_url_disables_caching() {
        assert!(!Cache::new(None).enabled());
        assert!(!Cache::new(Some("")).enabled());
        assert!(!Cache::new(Some("   ")).enabled());
    }

    #[test]
    fn accepts_urls_and_endpoints() {
        assert!(Cache::new(Some("redis://127.0.0.1:6379")).enabled());
        assert!(Cache::new(Some("127.0.0.1:6379")).enabled());
    }
}
