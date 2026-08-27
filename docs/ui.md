# UI architecture

Pkiboo workflows should not be coupled to a terminal. They describe work that
the application and operator must accomplish, while a UI backend decides how
that work is presented and how operator input is collected.

The initial backend is a conventional CLI. It renders tasks as terminal
progress indicators, tables as terminal tables, and should serialize requests
for operator input so prompts do not overlap. A future GUI could present the
same tasks as persistent cards, display several pending tasks at once, notify
the operator when attention is required, and resume each task independently.

This separation is particularly useful for Pkiboo because many workflows are
long-lived and depend on physical actions:

```text
workflow                         UI backend
----------------------------    --------------------------------------
request a destination medium -> CLI prompts and waits
                                 GUI displays an actionable task card

report discovery progress    -> CLI updates a spinner
                                 GUI updates status in the same card

ask for certificate fields   -> CLI asks one question at a time
                                 GUI presents a validated form

request final approval       -> CLI prints a summary and asks yes/no
                                 GUI presents a review dialog
```

The workflow owns the meaning and ordering of an operation. The backend owns
its visual layout and interaction style.

## Current implementation

The UI abstraction is re-exported from `src/ui/mod.rs`. Task traits and the
backend-neutral task tree live in `src/ui/task.rs`, with the CLI backend in
`src/cli_common.rs`.

`TaskStarter` is the common capability shared by UIs and tasks. It provides
`start_task` and an associated backend-specific task handle. Starting a task
from a UI creates a root task; starting one from a task creates its child.

`Presenter` is the common structured-output capability shared by UIs and
tasks. It presents backend-neutral property lists and tabular list models.
Domain concepts such as media backups remain outside this trait; a Pkiboo
extension translates those concepts into generic presentation models.

`PaneStarter` is the common presentation-grouping capability shared by UIs and
tasks. `start_pane` exposes a pane handle, while the `PaneStarterExt::pane`
helper mirrors the task helper by passing that handle into an async operation
and finalizing the pane afterward. A pane has a title and implements
`Presenter`, but deliberately has no progress or terminal state.

Pane operations may run concurrently. The CLI assigns panes creation-order
identities, buffers their structured output independently, and flushes complete
panes in that order. A graphical backend can instead expose all panes at once
as cards, panels, or tabs and populate each as its operation produces output.

`Ui` extends `TaskStarter`, `PaneStarter`, and `Presenter` and additionally
provides `ready`, which waits until the backend is ready.

A `Task` handle can:

- change its status message;
- mark itself complete;
- mark itself failed with an error message;
- mark itself cancelled;
- create presentation-only panes;
- present property lists and tabular lists through `Presenter`.

`TaskStarterExt` extends `TaskStarter`, rather than `Ui`, so its `task` helper works on
both a root UI and a running task. It creates a task, passes a cloneable handle
into the future, and automatically marks the task complete or failed according
to the result. This keeps lifecycle bookkeeping out of each workflow and makes
nested operations natural.

The CLI backend implements tasks with `indicatif::MultiProgress`, aligned
property cards, and lists with `comfy_table`. Both root and task output is
printed through the multi-progress renderer so it does not corrupt live bars.

The CLI maintains tasks in depth-first tree order. A new child is inserted after
its parent's existing descendant subtree and is indented according to its tree
depth. Completed tasks are removed from the live tree and printed once above
the remaining progress bars. Indicatif renders an ordered flat collection of
bars; Pkiboo supplies and maintains the hierarchy.

The abstraction is backend-generic, but still small. In particular:

- there is no active input API; an earlier prompt design is commented out;
- lists are output-only and contain strings rather than typed cells;
- warnings in `cli_common` bypass `Ui` and write directly to stderr;
- workflows cannot express retry or a request that is waiting specifically for
  operator action;
- presentation and interaction are not fully separated yet.

The implementation also already permits multiple asynchronous tasks to exist
at once. For example, key creation uses `try_join_all` while writing copies to
media, and the CLI backend uses a multi-progress renderer. The CLI should only
serialize interactions that require exclusive terminal input; it does not need
to serialize background work or status updates.

## Tasks and interactions

The UI should distinguish a **task** from an **interaction**.

A task represents ongoing work and observable state:

```text
pending -> running -> waiting for input -> running -> completed
                                             \-----> failed
                                             \-----> cancelled
```

An interaction is a typed request for an answer from the operator. It belongs
to a task, suspends the portion of the workflow that needs the answer, and
resolves asynchronously.

This means a workflow can naturally express:

```rust,ignore
let medium = task
    .select(SelectRequest::new("Choose destination media", choices))
    .await?;

task.set_message("Waiting for selected media").await;
backend.wait_for_available().await?;
```

The CLI interaction scheduler can acquire a single terminal-input lock, render
one request, read its answer, and then release the lock. Other tasks may keep
running and updating their status meanwhile. A GUI backend need not take that
lock and can expose multiple outstanding interactions simultaneously.

The serialization policy belongs to the backend. Workflows should not depend
on which prompt happens to reach the operator first when independent tasks run
concurrently.

## Proposed interaction primitives

The initial API should use a small number of typed primitives rather than a
generic `prompt(String) -> String`. Typed requests let each backend provide
appropriate controls and let validation remain consistent.

### Text input

Single-line text for names and certificate subject fields.

Useful options include:

- label and longer help text;
- initial or default value;
- placeholder;
- required versus optional;
- validation and a human-readable validation error;
- normalization that is explicit rather than silently UI-specific.

The answer should distinguish cancellation from an empty value.

### Secret input

An input whose value must not be echoed, retained in task history, included in
logs, or accidentally cloned as an ordinary `String`. This is appropriate for
passphrases and backend credentials. Its result should use a secrecy wrapper.

A backend should be told that a field is secret by its type, not by inspecting
its label.

### Multiline text

Text such as certificate policy notes or PEM/CSR input. The CLI may open an
editor or accept input until an explicit terminator; a GUI may use a multiline
editor. File input should generally remain a separate primitive so that text
and filesystem permissions are not conflated.

### Confirmation

A boolean approval request with explicit severity:

- ordinary confirmation;
- warning acknowledgement;
- destructive-operation confirmation.

Dangerous confirmations may require the operator to type the exact media name
or device fingerprint rather than answer a generic yes/no question. A default
of `false` should be normal, and non-interactive mode must not infer approval.

### Single choice

Choose one typed value from a set of labeled options. This is useful for media,
keys, certificates, algorithms, and policy profiles. Choices should have stable
IDs and optional descriptions; workflows should never depend on the displayed
label being returned unchanged.

The CLI can use a numbered or searchable selector. A GUI can use radio buttons,
a combo box, or a searchable list.

### Multiple choice

Choose zero or more values, with optional minimum and maximum counts. This is
needed when selecting complete-copy destinations or a set of media to verify.

### Form

A group of typed fields submitted and validated together. Certificate subject
details are the clearest use case:

```text
Common name          [________________________]
Organization         [________________________]
Organizational unit  [________________________]
Country              [__]
State                [________________________]
Locality             [________________________]
Validity             [1y______________________]
```

On the CLI, a form can still be asked field by field. Grouping the fields in
the model allows a GUI to render them together and permits cross-field
validation before submission. Forms should support conditional fields without
embedding UI layout instructions in workflows.

### File selection

A typed request to open or save a file, with purpose, expected formats, and
whether the path must already exist. The CLI accepts a path; a GUI can display
a native picker. For sensitive output, the request should also state the
required safety constraints rather than relying only on a filename extension.

This widget must not become a path for silently writing private keys to local
storage. Pkiboo's workflow and storage policy remain authoritative.

### Media action

Physical media deserves a first-class interaction instead of being represented
only by a changing spinner message. The request can describe:

- the required media or acceptable alternatives;
- whether insertion, removal, or replacement is required;
- the expected contents or purpose;
- discovered candidates and why any candidate was rejected;
- whether the operator may rescan, choose another medium, or cancel.

For example:

```text
Action required: insert destination media

Waiting for one of:
  - root-office
  - root-home

Detected:
  - scratch-drive (not registered for this operation)
```

A GUI could turn this into a notification and allow the operation to remain
pending indefinitely. A CLI keeps the request in the foreground while the
device-discovery future continues to run.

An explicit removal interaction is important after synchronized writes. It
lets Pkiboo require removal before advancing to the next Shamir share and makes
the independent-destination ceremony visible in the workflow.

### Review and approval

Security-sensitive operations should present structured facts for review before
approval. Examples include a certificate to be issued, requested versus granted
extensions, a destructive media operation, and a recovery plan.

A review contains sections, properties, warnings, and optionally a diff. The UI
decides how to lay it out; the workflow decides which facts must be shown and
what strength of approval is required.

### Retryable error

Some failures are not terminal: the wrong medium was inserted, a share was
unreadable, or a remote media backend was temporarily unavailable. A task
should be able to report a structured problem and offer actions such as retry,
choose another medium, skip where policy permits, or cancel.

This should be distinct from `mark_error`, which ends the task.

### Progress

The unused `Progress` trait suggests a useful distinction between indeterminate
work and bounded work. A progress model should support:

- indeterminate activity;
- a current count and total;
- a current phase;
- an optional unit such as bytes, shares, or media;
- cancellation when the workflow supports it.

Progress is state on a task rather than a separate user interaction.

### Structured output and notifications

Property views, tables, warnings, informational messages, and produced
artifacts should be backend-neutral UI output. They should not call
`println!`/`eprintln!` outside the CLI backend.

Useful output primitives are:

- a typed property sheet;
- a typed table with stable column IDs and display labels;
- informational, warning, and security-warning messages;
- an artifact result, such as an issued certificate, with an optional suggested
  filename and MIME/encoding information;
- a notification that a background task now requires attention.

Typed values would let a GUI sort dates and statuses correctly and let the CLI
produce JSON or other machine-readable output without parsing display strings.

## Suggested API shape

The exact Rust types can evolve, but interactions should be asynchronous,
typed, and associated with tasks. One possible shape is:

```rust,ignore
trait Ui: TaskStarter {
    async fn ready(&self) -> Result<(), UiError>;
    async fn interact<I: Interaction>(&self, task: &Self::TaskHandle, request: I)
        -> Result<I::Output, InteractionError>;
    async fn present(&self, task: Option<&Self::TaskHandle>, output: Output)
        -> Result<(), UiError>;
}

trait TaskStarter {
    type TaskHandle: Task + Clone;

    async fn start_task(&self, spec: TaskSpec) -> Result<Self::TaskHandle, UiError>;
}

trait Interaction {
    type Output;
}
```

Alternatively, convenience methods can live on the task handle:

```rust,ignore
let subject = task.form(subject_request).await?;
let media = task.select(media_request).await?;
task.review(certificate_review).await?.require_approval()?;
```

Request types should own their labels, help, choices, defaults, and validation
rules. Their output types should carry domain values where practical, such as a
`Name<Media>` rather than a display string.

The UI layer should not own PKI policy. For example, it may enforce that a
country field is two characters because the request declares that constraint,
but the certificate workflow remains responsible for deciding whether the
field is permitted and what certificate extensions are granted.

## Interaction lifecycle

Each task should have a stable ID and an explicit state. Child tasks are useful
for ceremonies with visible phases:

```text
Create key
├── Generate key material                 complete
├── Write copy to root-office           waiting for media
├── Write copy to root-home             running
└── Commit inventory                    pending
```

A task interaction should follow these rules:

1. The workflow creates a task and starts work.
2. When an answer is required, it submits a typed interaction associated with
   that task and awaits the result.
3. The backend exposes the interaction and eventually resolves it with an
   answer, cancellation, disconnection, or UI error.
4. The workflow validates domain and policy constraints. If correction is
   possible, it submits a revised interaction with validation feedback.
5. The workflow resumes and ultimately marks the task complete, failed, or
   cancelled.

Dropping a task handle should not silently report success. A guard may mark an
unfinished task as abandoned, but workflows should explicitly choose their
terminal state whenever possible.

## CLI behavior

The CLI backend should preserve a clean split between human and machine output:

- progress, prompts, warnings, and status belong on stderr;
- requested artifacts and machine-readable output belong on stdout;
- prompts are disabled when the relevant input stream is not interactive,
  unless answers were supplied through command options or an explicit input
  mechanism;
- an absent interactive answer is an error, not permission to choose a risky
  default;
- only one terminal-reading interaction is active at a time;
- background tasks may continue and the multi-progress display may continue to
  update while the interaction scheduler arbitrates prompt access.

Command-line arguments and UI interactions are two ways to provide the same
workflow inputs. A command can resolve each value in this order:

```text
explicit command option -> safe default -> UI interaction -> error if unavailable
```

Security-sensitive values should omit the safe-default step unless policy
defines one explicitly.

## GUI implications

A GUI backend should not require workflow changes. It can:

- retain completed and failed tasks in an operation history;
- display task trees and concurrent progress;
- show all outstanding operator actions in an inbox;
- raise system notifications for media insertion or approval;
- render typed forms and validation errors inline;
- allow a task to be cancelled when the workflow declares cancellation safe;
- persist enough non-secret task state to reconnect to long-running work.

Backend independence does not mean that every backend must look or behave the
same. It means every workflow expresses enough semantic information for each
backend to provide an appropriate experience without parsing terminal strings
or reimplementing PKI decisions.

## Recommended next steps

The smallest useful expansion is:

1. Add typed `confirm`, `text`, `select`, and `multi_select` interactions.
2. Add a CLI interaction scheduler that serializes terminal reads while allowing
   background tasks to continue.
3. Replace direct warning and property output with backend-neutral output
   primitives.
4. Model media insertion/removal as a first-class interaction.
5. Add forms, review/approval, file selection, and retryable errors as concrete
   workflows require them.

This is enough to replace the current `todo!()` sites for selecting media and
collecting certificate fields without prematurely designing an entire GUI
framework.
