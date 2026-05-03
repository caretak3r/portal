Output docs/images/tui-quick-switch.gif

# Demo: fuzzy quick-switch. `/` opens the picker, type a partial name, Enter loads.
# Phase 4. Faster than Tab-cycling once you have more than ~10 profiles.

Set Theme "Catppuccin Mocha"
Set FontSize 13
Set Width 1100
Set Height 620
Set Padding 12
Set TypingSpeed 50ms

Hide
Type "export HOME=/tmp/portal-demo-home" Enter
Type "export PATH=/Users/rohit/Documents/portal/target/release:$PATH" Enter
Type "clear" Enter
Show

Type "portal"
Enter
Sleep 1200ms

# Open the picker.
Type "/"
Sleep 800ms

# Recency order: personal-webdev was loaded last by the seed script, so it
# floats to the top with no query. Type "red" to filter to work-redteam —
# only one match.
Type "red"
Sleep 1200ms

# Enter loads. Async progress modal flashes; back to Detail with the new
# profile active.
Enter
Sleep 1800ms

# Try again with a different fragment.
Type "/"
Sleep 600ms
Type "min"
Sleep 1000ms
Enter
Sleep 1500ms

Hide
Type "q"
Show
