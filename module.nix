{
  lib,
  config,
  pkgs,
  ...
}:

let
  inherit (lib)
    # keep-sorted start
    boolToString
    collect
    concatStringsSep
    getExe
    isBool
    isList
    isString
    listToAttrs
    mapAttrsRecursive
    mkDefault
    mkEnableOption
    mkIf
    mkOption
    mkPackageOption
    optionalAttrs
    pipe
    toUpper
    types
    # keep-sorted end
    ;

  inherit (types)
    # keep-sorted start
    attrs
    attrsOf
    nullOr
    path
    port
    str
    submodule
    # keep-sorted end
    ;

  cfg = config.services.ta-schedule;

  environment =
    (pipe cfg.settings [
      (mapAttrsRecursive (
        path: value:
        optionalAttrs (value != null) {
          name = "TA_SCHEDULE_${toUpper (concatStringsSep "_" path)}";
          value =
            if isList value then
              concatStringsSep "," value
            else if isBool value then
              boolToString value
            else
              toString value;
        }
      ))
      (collect (x: isString x.name or false && isString x.value or false))
      listToAttrs
    ])
    // cfg.environment;
in
{
  options.services.ta-schedule = {
    enable = mkEnableOption "ta-schedule";
    package = mkPackageOption pkgs "ta-schedule" { };
    inputFile = mkOption {
      description = "Input file name of server";
      type = path;
      example = "/path/to/schedule.xlsx";
    };
    scheduleSheetName = mkOption {
      description = "Name of input schedule sheet";
      type = str;
      default = "Room Allocation";
    };
    exchangeSheetName = mkOption {
      description = "Name of input exchange sheet";
      type = str;
      default = "Session Exchange";
    };
    environment = mkOption {
      description = "Additional environment vars to set";
      type = attrs;
      default = { };
    };
    settings = mkOption {
      description = "Settings (env vars) for ta-schedule";
      type = submodule {
        freeformType = attrsOf str;
        options = {
          listen = mkOption {
            description = "Listen address";
            type = str;
            default = "0.0.0.0";
            example = "127.0.0.1";
          };
          base_url = mkOption {
            description = "Base URL that the app is served from, seen from the outside";
            type = str;
            default = "http://localhost:8000";
            example = "https://ta.bartoostveen.nl";
          };
          port = mkOption {
            description = "Port";
            type = port;
            default = 8000;
            example = 12345;
          };
        };
      };
      default = { };
    };
    environmentFile = mkOption {
      description = "path to the environment file containing variables such as the private key";
      type = nullOr path;
      default = null;
      example = "/run/path/to/secret/environment/file.env";
    };
  };

  config = mkIf cfg.enable {
    services.ta-schedule.environment = {
      RUST_LOG = mkDefault "info,tower_http=debug";
    };
    systemd.services.ta-schedule = {
      description = "ta-schedule";
      wantedBy = [ "multi-user.target" ];
      inherit environment;
      serviceConfig = {
        Type = "simple";
        ExecStart = ''
          ${getExe cfg.package} -S \
            --file "${cfg.inputFile}" \
            --schedule-sheet-name "${cfg.scheduleSheetName}" \
            --exchange-sheet-name "${cfg.exchangeSheetName}"
        '';
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
