{
  config,
  lib,
  pkgs,
  ...
}:

let
  inherit (lib)
    # keep-sorted start
    getExe
    mkDefault
    mkEnableOption
    mkIf
    mkOption
    mkPackageOption
    types
    # keep-sorted end
    ;

  inherit (types)
    # keep-sorted start
    attrs
    nullOr
    path
    # keep-sorted end
    ;

  cfg = config.services.ical-proxy;
  toml = pkgs.formats.toml { };
in
{
  options.services.ical-proxy = {
    enable = mkEnableOption "ical-proxy";
    package = mkPackageOption pkgs "ical-proxy" { };
    settings = mkOption {
      description = "Configuration written to the service's TOML config file";
      inherit (toml) type;
      default = { };
      example = {
        port = 8080;
        redis_url = "redis://127.0.0.1:6379";
      };
    };
    configFile = mkOption {
      description = "Path to an existing TOML configuration file; takes precedence over settings";
      type = path;
      default = toml.generate "ical-proxy.toml" cfg.settings;
      example = "/etc/ical-proxy/config.toml";
    };
    environment = mkOption {
      description = "Additional environment variables to set";
      type = attrs;
      default = { };
    };
    environmentFile = mkOption {
      description = "Path to an environment file with additional variables";
      type = nullOr path;
      default = null;
      example = "/run/secrets/ical-proxy.env";
    };
  };

  config = mkIf cfg.enable {
    systemd.services.ical-proxy = {
      description = "ical-proxy";
      wantedBy = [ "multi-user.target" ];

      environment = {
        ICAL_PROXY_CONFIG = cfg.configFile;
        RUST_LOG = mkDefault "info";
      }
      // cfg.environment;

      serviceConfig = {
        Type = "simple";
        ExecStart = getExe cfg.package;
        EnvironmentFile = cfg.environmentFile;
        DynamicUser = true;
        Restart = "on-failure";
        AmbientCapabilities = "";
        CapabilityBoundingSet = "";
        LockPersonality = true;
        MountAPIVFS = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateMounts = true;
        PrivateTmp = true;
        ProtectClock = true;
        ProtectControlGroups = "strict";
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        ProtectSystem = "strict";
        RemoveIPC = true;
        RestrictAddressFamilies = "AF_INET AF_INET6 AF_UNIX";
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        SystemCallArchitectures = "native";
        UMask = 27;
      };
    };
  };
}
