Output docs/images/tui-toggle.gif

# Demo: instant toggle. Load A, load B, then Backspace to bounce back to A.
# Demonstrates the previous_profile state and the ⌫ status-bar hint.

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
Sleep 1500ms

# Tab to work-redteam (alphabetical: minimal, personal-webdev, research, work-redteam).
# Active is personal-webdev so we tab forward twice to land on work-redteam.
Tab
Sleep 400ms
Tab
Sleep 800ms

# Load it. The progress modal is brief on a tiny demo home, so the
# perceived flicker is real — that's the async load doing its thing.
Type "l"
Sleep 600ms
Enter
Sleep 1800ms

# Status bar should now read: Active: work-redteam | ⌫ personal-webdev | …
# Backspace toggles straight back to personal-webdev.
Backspace
Sleep 1800ms

# And once more — symmetric toggle.
Backspace
Sleep 1500ms

Hide
Type "q"
Show
