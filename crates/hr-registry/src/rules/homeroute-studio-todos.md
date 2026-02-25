# Studio Todo List — Regles d'utilisation

## Quand creer un todo list

- **TOUJOURS** creer un todo list avec `TodoWrite` quand la tache comporte 3 etapes ou plus
- **TOUJOURS** creer un todo list quand l'utilisateur donne une liste de choses a faire
- **NE PAS** creer de todo list pour les questions simples ou les modifications triviales (1-2 etapes)

## Comment utiliser TodoWrite correctement

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

### Nettoyer

- Quand toutes les taches sont `completed`, laisser la liste telle quelle (l'utilisateur voit le progres)
- Ne jamais supprimer les taches completees — elles montrent le travail accompli

## Persistance — OBLIGATOIRE

Apres **chaque** appel a `TodoWrite`, appeler immediatement l'outil MCP `todo_save` (serveur `studio`) avec le meme contenu :

```
todo_save(todos=[...memes todos...])
```

Quand tu reprends un travail ou apres une compaction de contexte, appeler `todo_load` pour recuperer la liste :

```
todo_load() → recuperer la liste sauvegardee
TodoWrite(todos=[...liste recuperee...])
```

### Workflow type

1. Analyser la demande de l'utilisateur
2. `TodoWrite` avec toutes les taches identifiees
3. `todo_save` avec les memes todos
4. Pour chaque tache :
   a. `TodoWrite` avec la tache en `in_progress`
   b. `todo_save`
   c. Executer la tache
   d. `TodoWrite` avec la tache en `completed`
   e. `todo_save`
5. Verifier que toutes les taches sont `completed`

## Bonnes pratiques

- Descriptions courtes et claires (max ~60 caracteres)
- Granularite moyenne : ni trop detaille, ni trop vague
- Exemples de bonnes taches : "Creer le composant LoginForm", "Ajouter les routes API /users"
- Exemples de mauvaises taches : "Faire le code", "Step 1"
