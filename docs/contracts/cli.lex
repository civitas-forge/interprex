The Command Line

    What this tool does at its command line, and what a caller may rely on. A human reader gets the output layer's templates and styles; this contract covers what an agent reads. One rule generates the rest: the next action has to be computable from one invocation's output — the exit code, the structured result, and errors that name the flag or command that resolves them. A human who cannot tell what happened guesses and tries something; an agent has to spend another invocation finding out.

1. Structured Output

    Every command takes --output json. An environment variable carries the default output mode, and --output on the command line beats it; the config layer does that layering.

    :: tbd :: What that variable is called, and where a provisioned environment gets its value.

    In every mode, stdout carries what the command was asked for and nothing else — in json mode, one parseable object. Progress, warnings and log lines go to stderr; json output carries no ansi escapes.

    Errors honor the same contract. In json mode an error is a structured object: a stable machine-readable code, the message, and the remedy — the exact flag, argument or command that resolves it. An error naming its fix costs an agent one retry; one that does not costs an exploratory round-trip.

2. Exit Codes

    The exit code is the first contract, read before any parsing. Three outcomes: 0, the operation succeeded; 1, the operation ran and found problems — a lint with findings, a fleet run with failing repos; 2, the invocation itself was wrong. Failure never exits 0.

3. Explicit Interactivity

    Interactivity is sugar over flags, never the only path: every prompt has a non-interactive equivalent. Prompts fire only under --interactive; without it, a would-be prompt is an error naming the equivalent. With --interactive and no tty on stdin, the command errors immediately rather than blocking on a read that cannot complete. Nothing pages or animates into a pipe.

    --watch requires --timeout. More generally, a command that blocks on external progress — a container becoming healthy, a remote run completing — carries a default timeout that --timeout overrides, and a timeout error reports the state reached so far. A command that never returns ends an agent's turn with nothing to read; a timeout error leaves it something to act on.

4. Bounded Output

    A command that runs other commands — writes each child's full stdout and stderr to files and prints a summary instead: counts, the failures with a one-line reason each, exit codes, and the paths of those files. The summary is what the next action is chosen from; the paths let an agent read a full record selectively when it must.

    Truncation is always marked: a bounded listing carries a truncation flag and the total, and list commands take --limit. Silent truncation is worse than large output — it is wrong output an agent believes complete.

    A command that writes artifacts — logs, results directories, reports — names their paths in its output. An artifact the output does not name does not exist for an agent.

5. Retry

    A command that brings something to a declared state re-runs without error: the second run finds the state already reached and does nothing, so an agent that lost the result of the first run just runs it again. A command that reclaims is the same rule from the other side: it re-applies its own keep tests, and a second run finds nothing left to reclaim. Everything that mutates takes --dry-run, reporting the would-be changes in the same output format as the real run.

6. Concurrency

    Safety comes from ownership, never locks. An environment owns its container and its workspaces; races arbitrate on the runtime's own uniqueness — container names, directory creation, a registry's version immutability. A command that observes another run's interference classifies the collision: already done with matching content is reported as success; the same key with different content is a structured error naming what was found. A command never blocks waiting for a peer to finish. When it loses a race it says so in its structured error rather than exiting 0, so the agent that lost knows another run got there first and can act on it.

7. Discoverability

    --help is what an agent reads before reaching for a doc, and usually instead of one. Every command and every flag is documented there, and every command carries an example. Beyond help, the config layer exports the config json-schema.

    :: tbd :: Whether the json output objects get a schema export too, and under which subcommand. Only the config schema is exported, and no subcommand prints either one.
