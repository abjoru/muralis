# Issue tracker: GitHub

Issues and PRDs for this repo live as GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. Use a heredoc for multi-line bodies.
- **Read an issue**: `gh issue view <number> --comments`, filtering comments by `jq` and also fetching labels.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`
- **Link parent ↔ child (native sub-issue)**: `gh sub-issue add <parent> <child>` (via `yahsan2/gh-sub-issue` extension; install with `gh extension install yahsan2/gh-sub-issue`). A textual `## Parent #N` reference in the body is NOT enough — the Sub-issues panel only appears when this link is established via API. Fallback when the extension is unavailable: `gh api graphql -f query='mutation { addSubIssue(input:{issueId:"<parent-node-id>", subIssueId:"<child-node-id>"}) { issue { number } } }'` — fetch node IDs with `gh api graphql -f query='query { repository(owner:"<org>",name:"<repo>"){ issue(number:<N>){id} } }'`.

Infer the repo from `git remote -v` — `gh` does this automatically when run inside a clone.

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`.
