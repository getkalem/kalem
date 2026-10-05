# Security

Kalem opens files from anywhere and installs plugins from the index or a
link: a plugin's component runs in Kalem's sandbox with the permissions
its manifest declares (the Book, Part III, *Plugins*), and a download is
checked against the SHA-256 the index gives. A way around the sandbox, a
file that makes Kalem run code or write where it should not, or a flaw
in how plugins are fetched and checked is a security problem.

Please report one privately, through GitHub's *Report a vulnerability*
on this repository's Security tab, rather than in a public issue. Say
what an attacker gets and how to reproduce it; a fix is released as soon
as it is ready, and the report is credited unless you ask otherwise.
Only the latest release is supported.
