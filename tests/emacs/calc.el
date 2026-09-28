;;; calc.el --- Evaluate Calc formulas as Org table formulas do -*- lexical-binding: t -*-
;; Usage: emacs -Q --batch -l tests/emacs/calc.el IN OUT
;; IN has one formula per line; OUT gets FORMULA TAB RESULT lines, the
;; result being what `calc-eval' returns with `org-calc-default-modes'
;; (errors as ERROR).
(require 'org)
(require 'org-table)
(require 'calc)
(let* ((in (nth 0 command-line-args-left))
       (out (nth 1 command-line-args-left))
       (lines (with-temp-buffer
                (insert-file-contents in)
                (split-string (buffer-string) "\n" t))))
  (with-temp-file out
    (dolist (l lines)
      (let ((r (condition-case nil
                   (calc-eval (cons l org-calc-default-modes))
                 (error '(0 "lisp error")))))
        (insert l "\t" (if (stringp r) r "ERROR") "\n")))))
(setq command-line-args-left nil)
