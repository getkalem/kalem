;;; The expected results of Kalem's Babel comparison test, from Emacs's own
;;; Org: every source block of the file run in order, as C-c C-c runs it,
;;; and the file written back. Run as
;;;   emacs -Q --batch FILE -l tools/babel-oracle.el
;;; Confirmation is off (the test asks nothing); shell and Python are
;;; loaded; Python is `python3'. The blocks are found first, so that a
;;; block a result writes (`:results code') is not run in turn.
(require 'org)
(require 'ob-shell)
(require 'ob-python)
(setq org-confirm-babel-evaluate nil
      org-babel-python-command "python3"
      shell-file-name "/bin/sh"
      python-indent-guess-indent-offset-verbose nil)
(org-babel-do-load-languages 'org-babel-load-languages
                             '((shell . t) (python . t)))
(org-mode)
(let (blocks)
  (org-babel-map-src-blocks nil
    (push (copy-marker (match-beginning 0)) blocks))
  (dolist (m (nreverse blocks))
    (goto-char m)
    (org-babel-execute-src-block)))
(write-region (point-min) (point-max) (buffer-file-name))
