Output docs/images/tui-main.gif

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

# Pan the file tree on the active profile.
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 800ms

# Tab to a different profile and load it. The async-progress modal
# (Phase 3) flashes briefly before we land back in Detail.
Tab
Sleep 600ms
Type "l"
Sleep 800ms
Enter
Sleep 2200ms

# Phase 1 hint — status bar now reads "⌫ <previous>". Backspace
# toggles straight back without re-selecting from the list.
Backspace
Sleep 1800ms

Hide
Type "q"
Show
