# Studio Todo List — Regles d'utilisation

## Quand creer un todo list

- **TOUJOURS** creer un todo list avec `TodoWrite` quand la tache comporte 3 etapes ou plus
- **TOUJOURS** creer un todo list quand l'utilisateur donne une liste de choses a faire
- **NE PAS** creer de todo list pour les questions simples ou les modifications triviales (1-2 etapes)

## Comment utiliser TodoWrite

### Creer la liste

Appeler `TodoWrite` avec toutes les taches identifiees, chacune avec :
- `content` : description imperative (ex: "Ajouter le composant Header")
- `activeForm` : forme progressive (ex: "Ajout du composant Header")
- `status` : `pending` pour les taches a venir

### Mettre a jour en temps reel

- Marquer **exactement UNE** tache comme `in_progress` a la fois
- Marquer les taches `completed` **immediatement** apres les avoir terminees
- Ne **JAMAIS** marquer une tache `completed` si elle n'est pas reellement finie
- Si de nouvelles sous-taches apparaissent, les ajouter avec `pending`

### Iteration dans le meme chat

- Tant que l'activite est en cours : **garder** les taches completees visibles (l'utilisateur voit le progres)
- Quand toutes les taches sont `completed` et que l'utilisateur fait une **nouvelle demande** dans le meme chat : **mettre a jour et/ou nettoyer** la todolist en fonction des besoins — garder ce qui reste pertinent, supprimer ce qui ne l'est plus, ajouter les nouvelles taches
- Appeler `TodoWrite` + `todo_save` avec la liste mise a jour

## Phases (activites complexes)

Pour les activites importantes (5+ taches, etapes distinctes), organiser les taches en **phases**. Chaque phase contient sa propre todolist. Les phases ne sont pas toujours obligatoires — uniquement quand l'activite s'y prete.

### Convention TodoWrite (affichage)

Puisque `TodoWrite` n'accepte qu'un tableau plat, les phases sont representees par des entrees separatrices :

- Entree phase : `content` = `"▸ Phase N — Nom"`, `activeForm` = `"Phase N — Nom"`
- Les taches de la phase suivent immediatement apres l'entree phase
- Le `status` de l'entree phase reflete l'etat global de la phase :
  - `pending` — aucune tache demarree
  - `in_progress` — au moins une tache en cours ou terminee, mais pas toutes
  - `completed` — toutes les taches de la phase terminees

### Stockage via todo_save (structure)

Utiliser le parametre `phases` (pas `todos`) pour stocker la structure imbriquee :

```
todo_save(phases=[
  { "name": "Configuration", "status": "in_progress", "todos": [
    { "content": "Creer le fichier .env", "status": "completed", "activeForm": "Creation du fichier .env" },
    { "content": "Configurer la DB", "status": "in_progress", "activeForm": "Configuration de la DB" }
  ]},
  { "name": "Implementation", "status": "pending", "todos": [
    { "content": "Creer les composants React", "status": "pending", "activeForm": "Creation des composants React" },
    { "content": "Ajouter les routes API", "status": "pending", "activeForm": "Ajout des routes API" }
  ]}
])
```

### TodoWrite correspondant

```json
[
  {"content": "▸ Phase 1 — Configuration", "activeForm": "Phase 1 — Configuration", "status": "in_progress"},
  {"content": "Creer le fichier .env", "activeForm": "Creation du fichier .env", "status": "completed"},
  {"content": "Configurer la DB", "activeForm": "Configuration de la DB", "status": "in_progress"},
  {"content": "▸ Phase 2 — Implementation", "activeForm": "Phase 2 — Implementation", "status": "pending"},
  {"content": "Creer les composants React", "activeForm": "Creation des composants React", "status": "pending"},
  {"content": "Ajouter les routes API", "activeForm": "Ajout des routes API", "status": "pending"}
]
```

### Quand utiliser les phases

- **OUI** : refactoring multi-fichiers, nouvelle feature fullstack, migration de schema + code + tests
- **NON** : ajout d'un composant, correction de bug simple, taches lineaires < 5 etapes
- Limites : 2-4 phases max, 2-6 taches par phase

## Persistance — OBLIGATOIRE

Apres **chaque** appel a `TodoWrite`, appeler immediatement l'outil MCP `todo_save` (serveur `studio`) avec le meme contenu :

- Sans phases : `todo_save(todos=[...memes todos...])`
- Avec phases : `todo_save(phases=[...structure imbriquee...])`

Quand tu reprends un travail ou apres une compaction de contexte, appeler `todo_load` pour recuperer la liste :

```
todo_load() → recuperer la liste sauvegardee
```

Si le resultat contient `phases`, reconstruire la liste plate avec les entrees `▸ Phase N` pour `TodoWrite`.
Si c'est un tableau plat, l'utiliser directement.

### Workflow type

1. Analyser la demande de l'utilisateur
2. `TodoWrite` avec toutes les taches (et phases si complexe)
3. `todo_save` avec les memes todos (ou phases)
4. Pour chaque tache :
   a. `TodoWrite` — tache en `in_progress` (+ mettre a jour l'entree phase si applicable)
   b. `todo_save`
   c. Executer la tache
   d. `TodoWrite` — tache en `completed` (+ mettre a jour l'entree phase si toutes ses taches sont finies)
   e. `todo_save`
5. Verifier que toutes les taches sont `completed`

## Bonnes pratiques

- Descriptions courtes et claires (max ~60 caracteres)
- Granularite moyenne : ni trop detaille, ni trop vague
- Bonnes taches : "Creer le composant LoginForm", "Ajouter les routes API /users"
- Mauvaises taches : "Faire le code", "Step 1"
- Phases : 2-4 phases max, 2-6 taches par phase
