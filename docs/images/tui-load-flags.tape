Output docs/images/tui-load-flags.gif

# Demo: per-load flag chips + dry-run preview. Phase 5.
# Shift-L opens LoadConfirm with dry-run preset; lower-case l opens it with
# the safe defaults. b/p/d toggle each flag inline.

Set Theme "Catppuccin Mocha"
Set FontSize 13
Set Width 1100
Set Height 620
Set Padding 12
Set TypingSpeed 30ms

Hide
Type "export HOME=/tmp/portal-demo-home" Enter
Type "export PATH=/Users/rohit/Documents/portal/target/release:$PATH" Enter
Type "clear" Enter
Show

Type "portal"
Enter
Sleep 1200ms

# Tab to work-redteam.
Tab
Sleep 300ms
Tab
Sleep 800ms

# Shift+L = open LoadConfirm with dry-run preset on. Title flips to
# "Confirm Load — DRY RUN" and the chip row shows [x] d:dry-run.
Type "L"
Sleep 1500ms

# Toggle backup off and plugins off — pure preview now, no side effects.
Type "b"
Sleep 700ms
Type "p"
Sleep 1500ms

# Enter dispatches. Because dry-run is on, the loader is *not* spawned —
# the status bar just reports what would have happened.
Enter
Sleep 2200ms

# Now do a real load with the safe defaults: lower-case l, then Enter.
Type "l"
Sleep 1000ms
Enter
Sleep 2500ms

Hide
Type "q"
Show
