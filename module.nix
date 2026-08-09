{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.forebodere;
  user = "forebodere";
  group = user;
in
{
  options.services.forebodere = {
    enable = mkEnableOption (lib.mdDoc "Forebodere, a Discord quote bot.");

    environmentFile = mkOption {
      type = types.path;
      description = lib.mdDoc "Path to an env file providing DISCORD_TOKEN (e.g. a sops secret).";
      example = "/run/secrets/forebodere.env";
    };

    prefix = mkOption {
      type = types.str;
      default = "!";
      description = lib.mdDoc "Command prefix.";
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
        ExecStart = "${pkgs.forebodere}/bin/forebodere --db /var/lib/forebodere/forebodere.db --prefix ${cfg.prefix}";
      };
    };
  };
}
