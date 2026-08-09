.. image:: github-header.png
    :alt: forebodere
    :align: center

Forebodere is a quotation bot for Discord. Just for fun.
Deprecates my old `python`_ one.

Building
--------

To build::

  λ nix build

Running
-------

Forebodere takes the Discord bot token via the ``DISCORD_TOKEN`` environment
variable, and the path to a JSON configuration file via ``--config``::

  λ DISCORD_TOKEN='...' forebodere --config /path/to/config.json

Configuration
-------------

.. list-table::
   :header-rows: 1

   * - Option
     - Description
     - Default
   * - ``db``
     - Path to the SQLite database. **Required.**
     - *none*
   * - ``prefix``
     - The command prefix.
     - ``"!"``
   * - ``lol_quiet_gap_seconds``
     - Seconds of silence required before a laugh streak announcement is
       posted.
     - ``5``
   * - ``laugh_words``
     - Words that count towards a laugh streak (e.g. ``"lol"``, ``"lmao"``).
       Repeats and case are handled automatically.
     - ``[]``
   * - ``reactions``
     - A list of ``{ "phrase": ..., "emoji": ... }`` pairs. When a message
       contains ``phrase``, the bot reacts with the named custom guild
       emoji.
     - ``[]``
   * - ``lol_tier_messages``
     - The ``{ "low": ..., "medium": ..., "high": ... }`` messages posted
       at each laugh-streak tier.
     - ``"Low"`` / ``"Medium"`` / ``"High"``
   * - ``markov_default_order``
     - Chain order (1-5) used for ``!markov`` generation.
     - ``2``

Example::

  {
    "db": "/var/lib/forebodere/forebodere.db",
    "prefix": "!",
    "lol_quiet_gap_seconds": 5,
    "laugh_words": ["lol", "lmao", "rofl"],
    "reactions": [{ "phrase": "my wife", "emoji": "murk" }],
    "lol_tier_messages": {
      "low": "Multilol!",
      "medium": "Ultralol!",
      "high": "M-M-M-MONSTERLOL!"
    },
    "markov_default_order": 2
  }

NixOS Module
------------

Forebodere can also be installed as a NixOS module:

.. code-block:: nix

  {
    inputs.forebodere.url = "github:autophagy/forebodere-rs";

    outputs = { self, nixpkgs, forebodere }: {
      nixosConfigurations.yourhostname = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux"; # or whatever your system is
        modules = [
          ./configuration.nix
          forebodere.nixosModules.default
        ];
      };
    };
  }

It can then be enabled and configured like so:

.. code-block:: nix

  {
    services.forebodere = {
      enable = true;
      environmentFile = "/run/secrets/forebodere.env";
      settings = {
        prefix = "!";
        lol_quiet_gap_seconds = 5;
        laugh_words = [ "lol" "lmao" "rofl" ];
        reactions = [{ phrase = "my wife"; emoji = "murk"; }];
      };
    };
  }

``environmentFile`` should point to a file providing ``DISCORD_TOKEN``, such
as a runtime-decrypted `sops-nix`_ secret. ``db`` is set automatically from
the service's ``StateDirectory`` and cannot be overridden in ``settings``.

.. _sops-nix: https://github.com/Mic92/sops-nix
.. _python: https://github.com/autophagy/forebodere
