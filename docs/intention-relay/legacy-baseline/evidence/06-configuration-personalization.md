# Configuration and personalization evidence

## Proved routes

- `Sidebar config control → TogglePanel → ConfigEditor → configService → gateway → get_config/get_config_schema_flattened/get_config_toml/update_config/update_config_from_toml`.
- `Sidebar theme/locale controls → App shell → shellStore/Paraglide`.
- Rust config update paths validate, persist and replace stored `AppState.config`; config changes emit an event consumed by the frontend listener.

## Visible result

The user can open and edit schema-driven/raw TOML configuration, save it, change theme and switch EN/RU locale.

## Limit

Saving config does not establish live rebuilding of already-constructed agent compression, skills or prompt assembly.
