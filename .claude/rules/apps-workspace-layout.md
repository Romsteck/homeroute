# Layout des apps HomeRoute — règle invariante

Chaque app HomeRoute vit sous `/opt/homeroute/apps/{slug}/` avec un invariant strict sur la localisation des fichiers de contexte agent.

## INVARIANT

**Le workspace code-server d'une app est `{slug}/src/`**, pas `{slug}/` (cf. [web/src/pages/Studio.jsx](web/src/pages/Studio.jsx) et [web/src/components/Layout.jsx](web/src/components/Layout.jsx) : `?folder=/opt/homeroute/apps/{slug}/src`).

Tout ce qui doit être lu par l'agent Claude Code dans le Studio DOIT donc vivre **sous `src/`** :

| Fichier | Chemin canonique |
|---|---|
| Carnet de bord (agent-owned) | `{slug}/src/CLAUDE.md` |
| Config MCP (CLI compat) | `{slug}/src/.mcp.json` |
| Settings MCP + auto-approve | `{slug}/src/.claude/settings.json` |
| Règles always-on | `{slug}/src/.claude/rules/*.md` |
| Skills lazy-loaded | `{slug}/src/.claude/skills/<name>/SKILL.md` (+ ressources à côté si besoin) |

Les fichiers placés **au-dessus** de `src/` (directement dans `{slug}/`) sont **invisibles pour l'agent** et sont activement supprimés à chaque régénération par [`hr-apps::context::generate_for_app`](crates/orchestrator/hr-apps/src/context.rs) via `cleanup_legacy_parent_context`.

Le niveau `{slug}/` (au-dessus de `src/`) est réservé aux fichiers **runtime** :

- `{slug}/db.sqlite` — base SQLite managée
- `{slug}/.env` — variables d'env du process supervisé
- `{slug}/src/` — le workspace de l'agent

## Quand tu écris du code de provisioning ou de génération de contexte per-app

- **Cible `app.src_dir()`**, jamais `app.app_dir()`, pour tout fichier destiné à l'agent.
- Les templates de scaffold pour des fichiers que l'agent doit éditer/lire vont dans `src/` (cf. [`crates/orchestrator/hr-apps/templates/`](crates/orchestrator/hr-apps/templates/)).
- Si tu ajoutes un nouveau type de contexte per-app (nouvelle rule, nouvelle skill), écris-le dans [`context.rs::generate_for_app`](crates/orchestrator/hr-apps/src/context.rs) sous `src_claude_dir`, **jamais** sous `claude_dir` (parent).

## CLAUDE.md est agent-owned

`src/CLAUDE.md` est créé **une seule fois** au scaffold initial via `write_if_missing` avec un skeleton minimal. Il n'est **jamais régénéré** ensuite — l'agent en est propriétaire. Les infos dynamiques (stack, port, autres apps, DB) vivent dans `.claude/rules/app-info.md` qui, elle, est régénérée à chaque `AppUpdate`/`AppRegenerateContext`/boot.

Voir la rule dédiée générée dans chaque app : `src/.claude/rules/claude-md-upkeep.md`.

## Workspace-level (distinct du per-app)

Le Studio global `studio.mynetwk.biz` ouvre `/opt/homeroute/apps/` comme workspace. À ce niveau racine, les fichiers suivants sont **légitimes** :

- `/opt/homeroute/apps/CLAUDE.md` (vue globale, liste des apps)
- `/opt/homeroute/apps/.claude/settings.json` (MCP non-project-scoped)
- `/opt/homeroute/apps/.mcp.json`

Écrits par [`setup-studio.sh`](scripts/setup-studio.sh) et [`context.rs::generate_root`](crates/orchestrator/hr-apps/src/context.rs). Ne pas les confondre avec le niveau per-app.

## Violation et détection

Un fichier de contexte agent au niveau `{slug}/` (au-dessus de `{slug}/src/`) est un bug — il sera silencieusement supprimé au prochain `generate_for_app` mais il faut corriger la source (du code qui a écrit au mauvais niveau, ou un scaffold obsolète).

Check rapide :

```bash
ssh romain@10.0.0.254 'for s in /opt/homeroute/apps/*/; do
  slug=$(basename "$s")
  case "$slug" in .claude) continue ;; esac
  ls "$s"CLAUDE.md "$s".mcp.json "$s".claude 2>/dev/null | head && \
    echo "⚠ fichier agent au mauvais niveau dans $s"
done'
```

Si cette commande affiche quoi que ce soit : quelque chose écrit au mauvais niveau — à corriger dans le code source avant qu'un cycle régen ne le nettoie sournoisement.
