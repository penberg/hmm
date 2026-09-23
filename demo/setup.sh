#!/bin/sh
# Creates the small repository that demo.tape records hmm in.
set -e
dir=${1:-/tmp/wordfreq}
hmm ls -a 2>/dev/null | awk '$2 == "top" { print $1 }' | xargs -r hmm rm 2>/dev/null || true
rm -rf "$dir"
mkdir -p "$dir"
cd "$dir"
git init -q -b main
cat > wordfreq.py <<'EOF'
#!/usr/bin/env python3
"""Count how often each word appears in a file."""

import re
import sys
from collections import Counter


def count(text):
    return Counter(re.findall(r"[a-z']+", text.lower()))


def main():
    with open(sys.argv[1]) as f:
        counts = count(f.read())
    for word, n in counts.most_common():
        print(f"{n:6} {word}")


if __name__ == "__main__":
    main()
EOF
cat > README.md <<'EOF'
# wordfreq

Count how often each word appears in a file, and print the words
from the most common to the least:

    python3 wordfreq.py README.md
EOF
git add -A
git commit -qm "Count word frequencies"
# Let Claude edit and commit without asking, so the recording needs no
# keystrokes beyond the prompt. Kept out of git so it is not committed.
mkdir -p .claude
cat > .claude/settings.local.json <<'EOF'
{
  "permissions": {
    "allow": [
      "Edit",
      "Write",
      "Bash(python3:*)",
      "Bash(git status:*)",
      "Bash(git diff:*)",
      "Bash(git log:*)",
      "Bash(git add:*)",
      "Bash(git commit:*)"
    ]
  }
}
EOF
echo .claude/ >> .git/info/exclude
