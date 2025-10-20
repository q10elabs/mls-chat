# CLAUDE.md - Repository Rules and Guidelines

## Agent session persistence and context tracking

You must always create, update maintain a changelog file that tracks specifications, changes, decisions, and progress.
Do this also at the beginning of a task before searching any file or asking clarifying questions.

Update the file before and throughout a task to:

- Track specifications from the user
- Maintain awareness of ongoing tasks and implementation decisions
- Reference previous conversations through changelog files when relevant
- Track project evolution and architectural decisions over time

The file is placed in the `changelog/` directory with the naming pattern:

- **Format:** `YYYYMMDD-topic.md` (generate the timestamp using the shell command `date +%Y%m%d`)
- **Topic generation:** Auto-generate from the user's initial request
- **Example:** `20250814-claude-md-improvements.md`

The changelog file must include:

1. **Task Specification**: Clear description of the original request and scope
2. **High-Level Decisions**: Major architectural, technical, or strategic decisions made
3. **Requirements Changes**: Track when and how requirements are modified mid-conversation
4. **Files Modified**: List of all files created, modified, or deleted (no code diffs, just summaries)
5. **Rationales and Alternatives**: Why certain approaches were chosen over others
6. **Obstacles and Solutions**: Problems encountered and brief (1-line) solutions
7. **Current Status**: Progress tracking and next steps

Content Guidelines:

- **Include**: Decision rationales, file modification summaries, requirement changes, obstacles with solutions
- **Exclude**: Specific code diffs, redundant information, overly technical implementation details
- **Structure**: Flexible format optimized for the specific conversation type
- **Persistence**: Never delete changelog files after work completion

## High-level map of the repository structure for quick context

- `client/node`: client using Node.js
- `client/python`: client using Python
- `server`: server program (rust)

## Product vision for the project

This project aims to demonstrate how to use OpenMLS to implement a
simple multi-user chat application in the terminal.

See the file `README.md` for details.

## Require clarification and plan approval before making code changes

Before making any code changes other than the changelog, you must follow this two-step process:

### Step 1: Ask Clarifying Questions
- Always ask at least one clarifying question about the user's request
- Understand the full scope and context of what they're asking for
- Clarify any ambiguous requirements or edge cases
- Ask about preferred approaches if multiple solutions exist
- Confirm the expected behavior and user experience

### Step 2: Present Implementation Plan
- After receiving clarification, present a detailed implementation plan
- Break down the work into specific, actionable steps
- Identify which files will be created, modified, or deleted
- Explain the technical approach and any architectural decisions
- Highlight any potential risks, trade-offs, or dependencies
- Estimate the complexity and scope of changes
- **Wait for explicit user approval** before proceeding with any code changes

### Approval Requirements
- User must explicitly approve the plan with words like "yes", "approved", "proceed", "go ahead", or similar
- If the user suggests modifications to the plan, incorporate them and seek re-approval
- Do not assume silence or ambiguous responses mean approval

### Exceptions
- This process may be skipped only for trivial changes like fixing obvious typos or formatting
- When in doubt, always follow the full process rather than assuming an exception applies

### Example Flow
1. User: "Add a login form to the app"
2. Assistant: "I'd like to clarify a few things about the login form: [questions]"
3. User: [provides answers]
4. Assistant: "Based on your requirements, here's my implementation plan: [detailed plan]. Does this approach look good to you?"
5. User: "Yes, that looks good"
6. Assistant: [proceeds with implementation]

## Code organization for the `quint-app` directory

- Data model (entities only) in `src/db/models`
- Service layer is split into: stores (`src/db/stores`, data persistence), business logic (`src/db/services`: `core`, `workflow`, `actions`), and presentation (`src/db/presentation`, UI-specific services).
- Stores: Only handle raw data persistence/retrieval, no business logic, direct DB/storage access, simple accessors return undefined/empty, "must" accessors throw on missing.
- Core services: Implement business rules/constraints, handle auth/validation, coordinate stores, use exceptions for errors.
- Workflow services: Manage process flows/state transitions, coordinate operations, handle bootstrapping/init, use exceptions for errors.
- Action services: Handle user actions/events, manage action-specific business logic.
- Presentation services: Transform data for UI, handle UI-specific state/logic, no business rules, no direct store access, translate exceptions to Result.
- Dependency direction: Presentation → Business → Stores. No circular dependencies.
- Business services do not know about UI; presentation does not access stores directly; stores do not contain business logic.
- Each layer has clear responsibility: business = "what can be done", presentation = "how it's shown", stores = "where it's stored".
- Presentation services handle exception translation for UI.
- Benefits: clearer organization, easier testing, better separation of concerns, maintainability, easier debugging.

The codebase uses a strict service layer separation: stores handle raw data persistence, core services implement business logic and rules, and presentation services transform data for the UI. Presentation services (e.g., CircleDisplayService, PatternDisplayService) do not access stores directly but instead receive all required dependencies via their constructors. The RootStore is responsible for instantiating and wiring up all stores and services, ensuring that each layer only depends on the appropriate lower layer. This enforces unidirectional dependencies and clear separation of concerns throughout the app.

## Tech stack for the quint-app application

Tech stack for the web app in sub-directory `quint-app`:

- Frontend: React 19 + TypeScript, Vite 6.3.3, Tailwind CSS 4, ShadcnUI (New York preset, Lucide icons)
- Mobile: Capacitor 7.1.0 (iOS/Android), SQLite via @capacitor-community/sqlite (native), sql.js (web)
- State Management: MobX 6.13.7 (RootStore, domain stores, model-store-service pattern, React context providers)
- Testing: Jest 29.7.0 (unit/integration, jsdom), Playwright for E2E (Chromium/Firefox, ESM, aria-labels, localStorage, theme, loading state)
- Backend/Auth: PocketBase 0.25.2 for authentication; invite code required for registration
- Notifications: react-hot-toast
- Logging: loglevel (centralized, runtime configurable, debug screen)
- Error/Performance Monitoring: Sentry (browser, react, replay, vite-plugin, debug screen)
- Key Utilities: @capacitor/share, custom shareUtils.ts, centralized logger, error UI, toast notifications
- Build/Config: Vite plugins (React, Tailwind, Sentry, rollup visualizer), HTTPS dev server, API proxy, CORS for Sentry, custom chunking
- TypeScript: Strict mode, ES2020/2022, isolated modules, separate configs for app/node
- ESLint: Prettier, React, React Hooks, Refresh plugins
- File Structure: src/ (api, components, screens, db, assets, styles, config, utils), ios/, android/, public/, dist/
- Database Layer: Platform-agnostic service, parameterized queries, three-stage init, asset-based deployment, unified error handling
- UI Patterns: Modular, utility-first, dark mode, responsive, state-based styling, fixed nav/button bars, centralized routes
- Dev Scripts: npm run dev, build, lint, preview, test, test:watch
- Playwright: E2E tests in tests/smoke/, cross-browser, environment switching, accessibility, theme/localStorage testing.

## Offer to run TypeScript linter e.g. after updating .ts or .tsx files

After modifying .ts or .tsx files, offer/remind the user to run
linters to check the code quality. Wait for the user to confirm before
running them yourself.

When requested or approved explicitly:

- Run `npx eslint . --fix` in the `quint-app` directory for the main web app
- Run `npm run lint` to check all errors have been fixed.
- Address remaining errors if you can, or ask the user for further instructions.

## Git workflow and commit practices including commit message formatting

Git Operation Rules:

- **User-initiated only**: Perform git operations only when explicitly prompted by the user
- **No automatic staging**: Never add files to the git index; always prompt the user to stage files manually
- **Command suggestions**: Provide exact git commands for the user to execute
- **Branch management**: User manages all branching operations manually

Commit Message Structure:

When prompted to generate commit messages, use this three-section format:

```
First line: [one line summary of change]

Previous: [Feature-specific description of the state before changes,
written as multi-line paragraphs describing what existed and how it
worked, focusing on the functionality being modified]

Changed: [High-level summary derived from `git diff --cached`,
describing what was modified, added, or removed in terms that connect
to the changelog file's decisions and rationales]

See: changelog/YYYYMMDD-topic.md
```

**Format Requirements:**

- First line is a condensed summary
- Maximum 80 characters per line
- Maximum 50 lines total
- Multi-line paragraphs for Previous and Changed sections
- Changelog reference at the end

Suggest commits when:

- A logical unit of work is complete (feature, bug fix, refactor)
- After implementing a planned step from an approved implementation plan
- Before switching to a different type of work (e.g., from implementation to testing)
- After resolving a significant obstacle or decision point
- When multiple files have been modified for a coherent change
- Before making experimental changes that might need to be reverted

Commit Message Generation Process:

1. Run `git diff --cached` in the project root directory to analyze staged changes
2. Reference the corresponding changelog file for context and rationales
3. Identify the feature/functionality being modified (Previous section)
4. Summarize the high-level changes (Changed section)
5. Format according to the 80-character, 50-line structure
6. Include changelog reference
7. Ensure the first line is a summary of the whole change
8. Execute `git commit` in the project root directory. IMPORTANT: do not run `git add`.

## High-level code documentation

Maintain an explanatory comment at the top of each source file that
provides an overview of the main items defined in that file. Update
this comment when updating the rest of the file.

## UI Implementation Guidelines
Guidelines to follow for all UI changes

- The user prefers to reuse the existing @BackButton.tsx component for back buttons rather than implementing a custom back button.

- Error handling in the codebase is standardized using the `handleErrors` utility from @Errors.ts. Service and presentation layer methods that may throw are wrapped in `handleErrors`, which returns a neverthrow `Result` type (ok or err). This allows UI components to handle errors in a consistent, type-safe way, showing user feedback (e.g., toast notifications) based on the `Result`. This approach avoids unhandled exceptions and centralizes error management.

---

## How to Create New Project Rules
When prompted to add new rules (.mdc), follow the instructions in .rules/how-to-create-rules.md
