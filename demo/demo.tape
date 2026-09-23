# Records Claude Code committing in a workspace, and hmm merging the commit.
#
#   demo/record.sh
#
# vhs writes only frames; record.sh turns them into .github/assets/demo.*.
# Claude's run is live, so each recording differs a little.

Output demo/frames/

Require hmm
Require claude

Set Shell zsh
Set FontSize 20
Set Width 1280
Set Height 720
Set Padding 28
Set Theme "Catppuccin Mocha"
Set TypingSpeed 25ms
Set CursorBlink false

Hide
Type "./demo/setup.sh /tmp/wordfreq && cd /tmp/wordfreq" Enter
Type "unset CLAUDECODE $(env | grep -o '^CLAUDE_CODE_[A-Z_]*')" Enter
Type "export PS1='%F{magenta}wordfreq%f $ ' && clear" Enter
Sleep 1s
Show

# record.sh speeds this up from here...
Type `hmm run -n top -w ~/.claude claude "Add a --top N flag to wordfreq.py and commit"` Sleep 300ms Enter
Wait+Screen@5m /· done/
# ...to here. What follows must last TAIL seconds in record.sh.
Sleep 2s
Ctrl+C
Ctrl+C
Hide
Wait /^wordfreq \$ *$/
Type "clear" Enter
Sleep 300ms
Show

Type "hmm merge top" Sleep 200ms Enter
Sleep 1.2s
Type "git log --oneline" Sleep 200ms Enter
Sleep 2.5s
