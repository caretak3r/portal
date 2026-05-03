#!/usr/bin/env bash
#
# seed-demo-home.sh — build a throwaway $HOME at /tmp/portal-demo-home so the
# VHS tapes in this directory have realistic content to record against.
#
# Idempotent: blows the demo home away on every run and rebuilds it from
# scratch. The .tape files reference this layout (portal load personal-webdev,
# portal diff work-redteam personal-webdev, …), so any new profile you add
# here should also show up in those tapes.
#
# Run from the repo root (or anywhere — paths are absolute):
#
#   ./docs/images/seed-demo-home.sh
#   vhs docs/images/tui-main.tape
#
set -euo pipefail

DEMO_HOME="/tmp/portal-demo-home"
PORTAL_BIN="$(cd "$(dirname "$0")/../.." && pwd)/target/release/portal"

if [[ ! -x "$PORTAL_BIN" ]]; then
    echo "error: $PORTAL_BIN not found — run \`cargo build --release\` first." >&2
    exit 1
fi

# Reset.
rm -rf "$DEMO_HOME"
mkdir -p "$DEMO_HOME/.claude"
export HOME="$DEMO_HOME"
export XDG_CONFIG_HOME="$DEMO_HOME/.config"

# Each helper stamps ~/.claude/ with a recognisable shape, then snapshots it
# as a profile. The profiles end up content-addressed under
# ~/.config/portal/profiles/<name>/.

stamp_profile() {
    local name="$1" claude_md_body="$2"; shift 2
    rm -rf "$HOME/.claude"
    mkdir -p "$HOME/.claude/skills" "$HOME/.claude/rules" "$HOME/.claude/memory" \
             "$HOME/.claude/commands" "$HOME/.claude/agents" "$HOME/.claude/hooks"
    printf '%s\n' "$claude_md_body" > "$HOME/.claude/CLAUDE.md"
    printf '{"theme":"dark","autoUpdate":true}\n' > "$HOME/.claude/settings.json"
    # Each remaining arg is "subdir/filename:body" — quick way to seed several
    # files without repeating boilerplate.
    for spec in "$@"; do
        local rel="${spec%%:*}" body="${spec#*:}"
        mkdir -p "$HOME/.claude/$(dirname "$rel")"
        printf '%s\n' "$body" > "$HOME/.claude/$rel"
    done
    "$PORTAL_BIN" save "$name" --force --quiet
}

stamp_profile "work-redteam" \
    "# Work — Red Team
Offensive security workflows. Burp, ffuf, semgrep rules." \
    "skills/recon.md:# Recon\nWalkthrough for external recon." \
    "skills/exploit-dev.md:# Exploit dev\nFuzzing, payload crafting, debug." \
    "rules/scope.md:# Scope rules\nRespect engagement scope at all times." \
    "memory/clients.md:# Client notes\n- ACME: SAML, OAuth\n- Foo: GraphQL"

stamp_profile "personal-webdev" \
    "# Personal — Webdev
Side projects. Next.js, Tailwind, shadcn/ui." \
    "skills/nextjs.md:# Next.js\nApp Router patterns and gotchas." \
    "skills/tailwind.md:# Tailwind\nDesign tokens, dark mode, animations." \
    "rules/style.md:# Style guide\n- Prefer composition over inheritance.\n- One file per component."

stamp_profile "research" \
    "# Research
Reading papers, taking notes, summarising findings." \
    "skills/literature-review.md:# Lit review\nSearch, skim, synthesise." \
    "memory/papers.md:# Papers tracked\n- Attention is All You Need\n- Mamba\n- DPO"

stamp_profile "minimal" \
    "# Minimal
Just the essentials." \
    "rules/keep-it-small.md:# Keep it small\nNo skill bloat."

# Final state: leave personal-webdev active so demos that don't explicitly load
# something still have a real profile bound. Use --no-plugins to keep the
# recording offline (no `claude` binary involved).
"$PORTAL_BIN" load personal-webdev --no-plugins --force --quiet

echo "seeded $DEMO_HOME — profiles:"
"$PORTAL_BIN" list
