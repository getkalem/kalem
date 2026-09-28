;;; table.el --- Recalculate Org tables as Emacs does -*- lexical-binding: t -*-
;; Usage: emacs -Q --batch -l tests/emacs/table.el IN OUT
;; Every table of IN with a #+TBLFM line is recalculated once
;; (`org-table-recalculate' with a prefix: all rows).  OUT gets, per
;; table, its fields row by row (RULE for rules, fields joined by TAB),
;; or ERROR and the message, then a blank line.
(require 'org)
(require 'org-table)
(require 'calc)
(defun kalem-dump-table ()
  (let ((lines '()))
    (save-excursion
      (goto-char (org-table-begin))
      (while (and (< (point) (org-table-end)) (looking-at "[ \t]*|"))
        (push (if (looking-at "[ \t]*|-")
                  "RULE"
                (mapconcat #'identity
                           (org-split-string
                            (org-trim (buffer-substring-no-properties
                                       (line-beginning-position) (line-end-position)))
                            " *| *")
                           "\t"))
              lines)
        (forward-line 1)))
    (nreverse lines)))
(let* ((in (nth 0 command-line-args-left))
       (out (nth 1 command-line-args-left))
       (results '()))
  (with-temp-buffer
    (insert-file-contents in)
    (setq default-directory (file-name-directory (expand-file-name in)))
    (org-mode)
    (goto-char (point-min))
    (while (re-search-forward "^[ \t]*|" nil t)
      (let ((end (save-excursion (goto-char (org-table-end)) (point-marker))))
        (when (save-excursion
                (goto-char end)
                (looking-at "\\([ \t]*\n\\)*[ \t]*#\\+TBLFM:"))
          (let ((msg (condition-case e
                         (progn (org-table-recalculate t t) nil)
                       (error (error-message-string e)))))
            (push (if msg
                      (list (concat "ERROR " (car (split-string msg "\n"))))
                    (kalem-dump-table))
                  results)))
        (goto-char end)
        (forward-line 1))))
  (with-temp-file out
    (dolist (r (nreverse results))
      (dolist (l r) (insert l "\n"))
      (insert "\n"))))
(setq command-line-args-left nil)
