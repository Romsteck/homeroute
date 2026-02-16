# HomeRoute Store (Publication d'apps)

Le serveur MCP `store` permet de publier et gérer des applications Android (APK) sur le HomeRoute Store.

## Règles
- **JAMAIS publier sur le Store sauf si l'utilisateur l'a explicitement demandé**
- En développement, les applications mobiles tournent en mode DEV (Flutter run ou Expo start) — ne pas builder d'APK sauf pour publication
- TOUJOURS builder l'APK AVANT de publier (`eas build` ou build Gradle local)
- Utiliser `list_store_apps` pour vérifier les apps existantes avant de publier
- Lors de la première publication, fournir obligatoirement: `name`, `slug`, `version`
- Incrémenter la version à chaque nouvelle release

## Outils disponibles
- `list_store_apps` — Lister toutes les apps disponibles dans le Store (noms, slugs, catégories, versions)
- `get_app_info` — Détails d'une app spécifique (versions, changelogs)
- `check_updates` — Vérifier les mises à jour disponibles pour des apps installées
- `publish_release` — Publier une nouvelle release APK sur le Store

## Procédure de publication
1. Builder l'APK (ex: `eas build --platform android --profile preview --local` ou `flutter build apk`)
2. Vérifier avec `list_store_apps` si l'app existe déjà
3. Publier avec `publish_release` en fournissant: `apk_path`, `slug`, `version`
4. Pour une première publication, ajouter aussi: `name`, `description`, `category`
5. Vérifier avec `get_app_info` que la release apparaît dans le Store
