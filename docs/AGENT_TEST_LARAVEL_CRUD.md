# Agent Test: Laravel CRUD

Standard end-to-end test for evaluating an LLM agent's ability to autonomously build a working web application using tool calls.

## Task

Give the agent a short, high-level prompt:

> Build a People CRUD Laravel app in `E:\repo\tmp_project`. Serve it on port 8081.

The agent must figure out and execute all steps on its own:

1. Check the target directory state
2. Install Laravel if not present (`composer create-project`)
3. Create model, migration, controller, views
4. Configure database (SQLite)
5. Generate APP_KEY (`artisan key:generate`)
6. Run migrations
7. Start a background dev server
8. Verify it works

## Environment

| Item | Path / Value |
|------|-------------|
| PHP binary | `C:\php\php.exe` |
| Composer | `C:\php\php.exe C:\php\composer-real.phar` |
| Artisan | `C:\php\php.exe E:\repo\tmp_project\artisan` |
| Target dir | `E:\repo\tmp_project` |
| Dev server port | 8081 |
| Test URL | `http://localhost:8081/people` |

## Available Tools

The agent has access to: `execute_command`, `write_file`, `read_file`, `list_directory`, `execute_python`, `web_fetch`, `web_search`, `check_background_process`.

## Evaluation Criteria

| # | Criterion | Pass condition |
|---|-----------|---------------|
| 1 | Project setup | Laravel installed and bootable |
| 2 | Database | SQLite configured, migrations run, `people` table exists |
| 3 | Model | `Person` model with `name`, `email`, `phone` fillable fields |
| 4 | Controller | `PersonController` with all 7 resource methods |
| 5 | Routes | `Route::resource('people', ...)` in `web.php` |
| 6 | Views | Index, create, edit, show views (blade templates) |
| 7 | Server | PHP dev server running on port 8081 |
| 8 | CRUD works | Can create, read, update, delete a person via browser |

## Pre-test Cleanup

Empty the target directory before each test:

```
rm -rf E:/repo/tmp_project/*
```

Or if it's a git repo: `git checkout -- . && git clean -fd`

## Known Failure Modes

- **APP_KEY generation**: Small models try to write a literal base64 key and enter a repetition loop. Smart models use `artisan key:generate`.
- **artisan migrate hangs**: If the command waits for input (no `--no-interaction`), the 30s inactivity timeout kills the process tree. The agent sees the timeout message and should retry or adapt.
- **Composer output flood**: `composer create-project` produces 200+ lines. Output is sanitized (ANSI stripped, truncated to 80 lines) before entering model context.
- **Duplicate migrations**: If the agent runs `make:model -m` and also writes a manual migration, `artisan migrate` may fail with "table already exists".

## Running the Test

1. Clean `E:\repo\tmp_project`
2. Load the model in the UI
3. Start a new conversation
4. Send the simple prompt
5. Do NOT intervene — let the agent work autonomously
6. After generation completes (EOS or round limit), verify `http://localhost:8081/people`
