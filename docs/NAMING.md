# Naming rules

Use the following exact application-facing names in new product code, user interfaces, documentation, package metadata, and service/configuration definitions:

| Surface | Required name |
| --- | --- |
| Application and package | `endfield-unix` |
| Shared GUI and CLI executable | `endfield-unix` |
| Tracking daemon executable | `endfield-unixd` |
| Linux user service | `endfield-unixd.service` |
| Configuration namespace | `endfield-unix` |

GUI and CLI are modes/subcommands of the same `endfield-unix` executable. Do not introduce `endfield-unix-gui`, `endfield-unix-cli`, `endfield-linux`, or `Endfield Optimizer` as new product or executable names.

The GitHub repository identity is currently `nghianguyen150612/Endfield-Linux` and may remain so until the owner renames it. That repository name is the only permitted new-use exception for `Endfield-Linux`; historical repository references may also retain it. Do not rename the GitHub repository as part of P001.

Configuration, state, cache, runtime, and IPC paths must be derived from platform-appropriate user directories and namespaced by `endfield-unix`. Do not hard-code a home directory. Service-manager-specific naming belongs in the platform adapter; `endfield-unixd.service` is the Linux user-service unit name.
