;;; The expected export of Kalem's Babel-on-export comparison test, from
;;; Emacs's own Org: the file exported as HTML, the body only, its blocks
;;; whose results the export writes run first (`org-export-use-babel').
;;; Run as
;;;   emacs -Q --batch FILE -l tools/babel-export-oracle.el > FILE.html
;;; Confirmation is off (the test asks nothing); shell is loaded, as
;;; `/bin/sh'.
(require 'org)
(require 'ob-shell)
(setq org-confirm-babel-evaluate nil
      shell-file-name "/bin/sh")
(org-babel-do-load-languages 'org-babel-load-languages '((shell . t)))
(princ (org-export-as 'html nil nil t))
