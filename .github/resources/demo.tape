# Records the README demo gif with VHS (https://github.com/charmbracelet/vhs).
#
#   cargo build --release
#   vhs .github/resources/demo.tape

Output .github/resources/demo.gif

Require bash

Set Shell bash
Set FontSize 18
Set Columns 106
Set Rows 28
Set Padding 8
Set TypingSpeed 180ms
Set CursorBlink false
Set Theme { "name": "mtui", "black": "#111415", "red": "#ef5b5b", "green": "#48e63a", "yellow": "#dfcd71", "blue": "#5f9bff", "magenta": "#c678dd", "cyan": "#56b6c2", "white": "#c9cdd1", "brightBlack": "#5a6165", "brightRed": "#ef5b5b", "brightGreen": "#48e63a", "brightYellow": "#f2df7a", "brightBlue": "#5f9bff", "brightMagenta": "#c678dd", "brightCyan": "#56b6c2", "brightWhite": "#eceff1", "background": "#1e2021", "foreground": "#c9cdd1", "selection": "#3a3f42", "cursor": "#c9cdd1" }

Hide
Type "rm -rf target/gifwork && mkdir -p target/gifwork && cd target/gifwork && clear && ../release/mtui --config config.json --make-config-if-none"
Enter
Sleep 500ms
Show

# startup
Sleep 1.6s

# read a few registers
Down@250ms 5
Sleep 1s

# help popup
Type "h"
Sleep 1.4s
Down
Sleep 1.35s
Escape
Sleep 400ms

# graph on the "frequency" input
Up@250ms 4
Sleep 400ms
Type "g"
Sleep 5.4s
Type "g"
Sleep 800ms

# jump to the "seconds" register by label
Type "j"
Sleep 1.25s
Type "sec"
Sleep 1.5s
Enter
Sleep 1.65s

# custom rule on "seconds": /60, 1 decimal, " min"
Type "m"
Sleep 1.35s
Down
Sleep 250ms
Type "/60"
Enter
Sleep 450ms
Down@250ms 3
Type "1"
Sleep 300ms
Down@250ms 2
Type " min"
Sleep 1.5s
Enter
Sleep 1.95s

# settings: Display -> Show frame render time
Type "s"
Sleep 1.2s
Down@250ms 2
Enter
Sleep 900ms
Down
Sleep 350ms
Right
Sleep 1.7s
Escape
Sleep 350ms
Escape
Sleep 2.45s

# about popup as the closing shot
Type "a"
Sleep 4.5s

Hide
Escape
Type "q"
