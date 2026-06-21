# Model Task Evaluation — 2026-06-20

**Models tested:** Qwen3.6-35B-A3B-UD-IQ4_XS (desktop, turboKV), Qwen 3.5 9B Q8 (web)  
**Backend:** llama_chat_app.exe (Tauri desktop) + llama_chat_web.exe (web server)  
**GPU:** RTX 4090 (24 GB VRAM)

---

## Task 1 — USA-Iran War Research

**Conversation:** `chat_2026-06-20-14-25-41-460`  
**Model:** 9B (web server, ~13 tok/s — slow due to long system prompt)  
**Output:** 6265 tokens | 13.1 tok/s  
**Result:** ✅ PASS

**What happened:**
- Used `browser_navigate` + `browser_get_text` to search Google, Al Jazeera, BBC, Reuters, AP
- Retrieved live blog content from Al Jazeera (JS-rendered sites blocked, only search snippets available)
- Produced a structured report covering: military timeline, ceasefire terms, nuclear dimension, economic impact, regional context (Lebanon/Hezbollah), sources consulted, and research limitations

**Key findings surfaced:**
- Feb–Jun 2025: US+Israel airstrikes on Iranian nuclear sites; Iran retaliated across the Gulf
- Mid-2025: Initial truce signed; 60-day ceasefire + 14-point MoU negotiated
- Jun 2026: Technical talks in Switzerland postponed; Israeli strikes in Lebanon jeopardizing deal
- Strait of Hormuz remained closed; ~$2.2T global economic damage
- Iran internally divided; Trump called it "unconditional surrender"

**Notes:** JS-rendered news sites (BBC, CNN, NYT, Reuters) returned errors — model correctly fell back to Google search snippets and Al Jazeera live blog.

---

## Task 2 — Laravel People CRUD with SQLite

**Conversation:** `chat_2026-06-20-14-38-14-454`  
**Model:** 9B (web server)  
**Output:** 6063 tokens | 25.9 tok/s  
**Result:** ✅ PASS

**What happened:**
- Created full Laravel People CRUD at `E:\repo\tmp_project\laravel-people-crud`
- Created: migration, Model, ResourceController (7 routes), 4 Blade templates (layout/index/create/edit), PersonSeeder

**Files created:**
| Component | File |
|---|---|
| Migration | `create_people_table.php` — id, name, email (unique), phone, address, timestamps |
| Model | `app/Models/Person.php` — fillable fields |
| Controller | `PersonController.php` — full RESTful (index/create/store/edit/update/destroy) + validation |
| Routes | `Route::resource('people', PersonController::class)` + root redirect |
| Layout | `resources/views/layouts/app.blade.php` |
| Index | Paginated table with Edit/Delete |
| Create/Edit | Forms with validation error display |
| Seeder | 5 sample records |

**Endpoints verified:** 7 RESTful routes (GET /people, GET /people/create, POST /people, GET /people/{id}/edit, PUT /people/{id}, DELETE /people/{id})  
Dev server running at http://127.0.0.1:8000/people with 5 seeded records.

---

## Task 3 — Django People CRUD with SQLite

**Conversation:** `chat_2026-06-20-14-45-22-681`  
**Model:** 9B (web server)  
**Output:** tokens N/A (not stored) | speed N/A  
**Result:** ✅ PASS

**What happened:**
- Django 5.2.12 already installed; created project at `E:\repo\tmp_project\django-people-crud`
- Used `python -m django startproject config . && startapp people`
- Created class-based views: ListView, CreateView, UpdateView, DeleteView
- Ran migrations, seeded 5 sample records

**People visible in browser:**
1. Alice Johnson — alice@example.com — 555-0101
2. Bob Smith — bob@example.com — 555-0102
3. Carol Williams — carol@example.com — 555-0103
4. David Brown — david@example.com
5. Eva Martinez — eva@example.com — 555-0105

Dev server at http://127.0.0.1:8000/ with list/create/edit/delete UI confirmed working via `browser_navigate`.

---

## Task 4 — Spring Boot 3 People CRUD with SQLite (35B Model)

**Conversation:** `chat_2026-06-20-22-32-36-632`  
**Model:** Qwen3.6-35B-A3B-UD-IQ4_XS (desktop app, turbo2/turbo3 KV, gpu_layers=40, context=114688)  
**Output:** 5933 tokens | 16.0 tok/s | context used: ~19.5K / 114.7K  
**Result:** ✅ PASS

**What happened:**
- Created all Spring Boot files at `E:\repo\tmp_project\spring-people-crud`
- Found Maven at `C:\Users\agus_\apache-maven\apache-maven-3.9.6` (not in PATH — model located it via `dir /s`)
- Found Java 21 (Eclipse Adoptium) used by Maven
- Hit repetition loop searching for mvn.cmd — anti-loop detection fired, model recovered
- Encountered duplicate main class issue (`peoplecrud` vs `springpeoplecrud` packages) — model fixed by deleting stale directory
- Ran `mvn clean compile` — BUILD SUCCESS
- Started Spring Boot (background, PID 11652) — ran on port 8081 (8080 occupied)
- Tested all 7 CRUD operations via curl

**Files created:**
| File | Description |
|---|---|
| `pom.xml` | Spring Boot 3.4.4 + JPA + Web + sqlite-jdbc + hibernate-community-dialects |
| `SpringPeopleCrudApplication.java` | Main entry point |
| `entity/Person.java` | JPA entity: id, name, email, phone |
| `repository/PersonRepository.java` | JpaRepository<Person, Long> |
| `service/PersonService.java` | Business logic |
| `controller/PersonController.java` | REST controller |
| `application.properties` | SQLite datasource, ddl-auto=update |

**CRUD test results (all ✅):**
| Endpoint | Method | Result |
|---|---|---|
| /api/people | GET | Empty list `[]` |
| /api/people | POST | Created 3 people (IDs 1, 2, 3) |
| /api/people/1 | GET | Returns John Doe Jr. |
| /api/people/1 | PUT | Updated name + phone |
| /api/people/3 | DELETE | HTTP 204 (removed Bob Wilson) |
| /api/people/999 | DELETE | HTTP 404 |
| /api/people/999 | GET | HTTP 404 |

**SQLite DB created automatically:** `people.db` (12KB) in project root.

---

## Summary

| Task | Model | tok/s | Output Tokens | Result |
|---|---|---|---|---|
| USA-Iran Research | 9B Q8 web | 13.1 | 6265 | ✅ PASS |
| Laravel CRUD | 9B Q8 web | 25.9 | 6063 | ✅ PASS |
| Django CRUD | 9B Q8 web | N/A | N/A | ✅ PASS |
| Spring Boot CRUD | **35B IQ4_XS desktop** | **16.0** | 5933 | ✅ PASS |

**4/4 tasks passed.** The 35B model ran at 16 tok/s (vs 25–50 tok/s for 9B) due to larger model size and higher context demands (114K context window). The 35B model showed better recovery behavior: it self-corrected the duplicate class issue, located Maven without PATH configuration, and hit the anti-repetition trigger once (mvn.cmd search loop) and recovered cleanly.

**Desktop app routing challenges encountered:**
- Multi-worker pool routes by agent_id; per-agent workers don't appear in `/api/model/status`
- Conversations created via REST API don't carry agent config to the Tauri IPC path
- Resolved by using `/api/chat/stream` SSE endpoint directly after loading 35B as the default worker
- Context size 262144 caused VRAM OOM (>37GB needed); reduced to 114688 to fit 24GB RTX 4090
