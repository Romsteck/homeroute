# HomeRoute Store (Publication d'apps)

Le serveur MCP `store` permet de publier et gerer des applications Android (APK) sur le HomeRoute Store.

## Environnement
- **App** : {{slug}}
- **Store** : accessible depuis le dashboard HomeRoute

## Regles
- **JAMAIS publier sur le Store sauf si l'utilisateur l'a explicitement demande**
- En developpement, les applications mobiles tournent en mode DEV
- TOUJOURS builder l'APK AVANT de publier
- Utiliser `list_store_apps` pour verifier les apps existantes avant de publier
- Lors de la premiere publication, fournir: `name`, `slug`, `version`

## Outils disponibles
- `list_store_apps` — Lister les apps du Store
- `get_app_info` — Details d'une app
- `check_updates` — Verifier les mises a jour
- `publish_release` — Publier une release APK

## Procedure de publication
1. Builder l'APK
2. `list_store_apps` — Verifier si l'app existe
3. `publish_release` — Publier avec `apk_path`, `slug`, `version`
4. `get_app_info` — Verifier la publication
