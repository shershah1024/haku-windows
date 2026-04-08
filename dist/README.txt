Haku for Windows
================

Local MCP server that exposes native Win32 apps and Chrome (via extension)
as tools for AI agents.

Quick start
-----------

  1. Right-click install.bat -> Run as administrator (or run from cmd)
     Installs haku.exe to %LOCALAPPDATA%\Haku and adds it to your user PATH.

  2. Open a new PowerShell window so PATH refreshes.

  3. Run setup:
       haku --setup
     Optionally downloads the EmbeddingGemma-300M model (~313MB) for
     semantic tool search. Without it, haku still runs — search just
     uses substring matching.

  4. Start the server:
       haku
     Listens on:
       - http://127.0.0.1:19820/mcp   (MCP JSON-RPC, requires bearer token)
       - ws://127.0.0.1:19822/ws       (Chrome extension bridge)
     Bearer token is at: %USERPROFILE%\.haku\config.json

  5. Connect a Chrome extension or MCP client to the above ports.

Files
-----

  haku.exe          The server binary (statically linked, ~8MB or ~12MB with
                    embedding feature).
  install.bat       Installs haku.exe to %LOCALAPPDATA%\Haku and updates PATH.
  uninstall.bat     Removes haku.exe (and optionally config + models).
  README.txt        This file.
  LICENSE.txt       License information.

Config locations
----------------

  %USERPROFILE%\.haku\config.json     Server config (token, port, license)
  %USERPROFILE%\.haku\models\         Embedding model directory
  %LOCALAPPDATA%\Haku\flows.db        Recorded UI flows (SQLite)
  %TEMP%\haku.log                     Server log

Commands
--------

  haku                       Start the server
  haku --version             Print version
  haku --help                Show help
  haku --setup               First-run setup (creates dirs, offers model dl)
  haku --download-model      Download EmbeddingGemma-300M GGUF (~313MB)
  haku --activate <KEY>      Activate a license key

License
-------

Haku ships with a 14-day trial. After that, activate a license:
  haku --activate XXXX-XXXX-XXXX-XXXX

Buy a license at: https://your-store-url-here

Logs
----

Server logs go to %TEMP%\haku.log. Set RUST_LOG=debug for more verbose output:
  set RUST_LOG=debug && haku
