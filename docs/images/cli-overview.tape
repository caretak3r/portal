Output docs/images/cli-overview.gif

Set Theme "Catppuccin Mocha"
Set FontSize 14
Set Width 980
Set Height 620
Set Padding 20
Set TypingSpeed 40ms
Set PlaybackSpeed 1.0

Hide
Type "export HOME=/tmp/portal-demo-home" Enter
Type "export PATH=/Users/rohit/Documents/portal/target/release:$PATH" Enter
Type "clear" Enter
Show

Type "portal list"
Sleep 600ms
Enter
Sleep 1500ms

Type "portal load work-redteam --no-plugins --force"
Sleep 600ms
Enter
Sleep 2200ms

# Phase 1: instant toggle to whichever profile was active before.
Type "portal toggle --no-plugins --force"
Sleep 600ms
Enter
Sleep 2200ms

Type "portal status"
Sleep 400ms
Enter
Sleep 2s
