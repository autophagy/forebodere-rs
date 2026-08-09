{
  config,
  lib,
  pkgs,
  ...
}:

with lib;

let
  cfg = config.services.forebodere;
  user = "forebodere";
  group = user;
  settingsFormat = pkgs.formats.json { };
  configFile = settingsFormat.generate "forebodere-config.json" (
    cfg.settings
    // {
      db = "/var/lib/forebodere/forebodere.db";
    }
  );
in
{
  options.services.forebodere = {
    enable = mkEnableOption (lib.mdDoc "Forebodere, a Discord quote bot.");

    environmentFile = mkOption {
      type = types.path;
      description = lib.mdDoc "Path to an env file providing DISCORD_TOKEN (e.g. a sops secret).";
      example = "/run/secrets/forebodere.env";
    };

    settings = mkOption {
      inherit (settingsFormat) type;
      default = { };
      description = lib.mdDoc ''
        Forebodere configuration, see <https://github.com/autophagy/forebodere-rs#configuration>.
        `db` is set automatically from the service's StateDirectory and cannot be overridden here.
      '';
      example = {
        prefix = "!";
        lol_quiet_gap_seconds = 5;
        laugh_words = [
          "lol"
          "lmao"
          "rofl"
        ];
        reactions = [
          {
            phrase = "my wife";
            emoji = "murk";
          }
        ];
        lol_tier_messages = {
          low = "Multilol!";
          medium = "Ultralol!";
          high = "M-M-M-MONSTERLOL!";
        };
        markov_default_order = 2;
      };
    };
  };

  config = mkIf cfg.enable {
    users.users.${user} = {
      inherit group;
      description = "Forebodere system user";
      isSystemUser = true;
    };

    users.groups = {
      forebodere = { };
    };

    systemd.services.forebodere = {
      description = "Forebodere Discord quote bot";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Restart = "on-failure";
        RestartSec = 5;
        User = user;
        Group = group;
        StateDirectory = "forebodere";
        EnvironmentFile = cfg.environmentFile;
        ExecStart = "${pkgs.forebodere}/bin/forebodere --config ${configFile}";
      };
    };
  };
}
