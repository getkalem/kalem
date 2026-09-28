;;; export.el --- Export Org files for Kalem's differential tests  -*- lexical-binding: t; -*-

;; Part of Kalem's differential testing (design document sections 10 and 16).
;; Licensed under MIT OR Apache-2.0.

;;; Commentary:

;; Usage:
;;
;;   emacs -Q --batch -l tests/emacs/export.el CASES-DIR OUTPUT-DIR
;;
;; Every CASES-DIR/NAME.org is exported, body only, with the html, md,
;; ascii and latex back-ends, to OUTPUT-DIR/NAME.html, NAME.md, NAME.txt
;; and NAME.tex.  The clock is fixed at 2026-09-28 Mon 10:00, the author
;; is "Kalem Tester" <tester@example.org>, and source blocks are not
;; colored (no htmlize), so that the results are the same everywhere.
;; With KALEM_EXPORT_FULL set, whole documents are exported instead of
;; bodies.  A case whose headline has a `KALEM_TEST_SUBTREE' property exports
;; only that subtree, as `C-c C-e C-s' does.

;;; Code:

(require 'org)
(require 'ox)
(require 'ox-html)
(require 'ox-md)
(require 'ox-ascii)
(require 'ox-latex)
(require 'org-inlinetask)

(defconst kalem-export-now (encode-time 0 0 10 28 9 2026)
  "The fixed time exports see.")

(setq user-full-name "Kalem Tester"
      user-mail-address "tester@example.org"
      org-html-htmlize-output-type nil
      org-export-with-broken-links 'mark
      org-resource-download-policy nil
      make-backup-files nil)

;; Kalem does not run Babel code: neither does this export.
(require 'ob)
(dolist (v '(org-babel-default-header-args
             org-babel-default-inline-header-args
             org-babel-default-lob-header-args))
  (set v (cons '(:eval . "never-export") (symbol-value v))))

(defvar kalem-export-backends '((html . "html") (md . "md") (ascii . "txt") (latex . "tex"))
  "The back-ends and the extensions of their files.")

(defun kalem-export-file (file out-dir &optional name)
  "Export FILE with each back-end into OUT-DIR, as NAME.EXT."
  (let ((name (or name (file-name-base file))))
    (dolist (b kalem-export-backends)
      (let ((out (with-temp-buffer
                   (insert-file-contents file)
                   ;; An absolute folder: links to other files resolve
                   ;; from it (`org-publish-resolve-external-link').
                   (setq default-directory (file-name-directory (expand-file-name file)))
                   (setq buffer-file-name file)
                   (org-mode)
                   (random "kalem")
                   (cl-letf (((symbol-function 'current-time)
                              (lambda (&rest _) kalem-export-now)))
                     (condition-case err
                         ;; A case with a `KALEM_TEST_SUBTREE' property
                         ;; exports that subtree only.
                         (let ((subtreep
                                (save-excursion
                                  (goto-char (point-min))
                                  (when (re-search-forward
                                         "^[ \t]*:KALEM_TEST_SUBTREE:" nil t)
                                    (org-back-to-heading t)
                                    (point)))))
                           (when subtreep (goto-char subtreep))
                           (org-export-as (car b) (and subtreep t) nil
                                          (not (getenv "KALEM_EXPORT_FULL"))))
                       (error (format "ERROR: %s\n" (error-message-string err))))))))
        (with-temp-file (expand-file-name (concat name "." (cdr b)) out-dir)
          (set-buffer-file-coding-system 'utf-8-unix)
          (insert out))))))

;; With a third argument `recursive', every Org file under CASES-DIR, named
;; after its path with `/' as `__'; the back-ends can be limited with
;; KALEM_EXPORT_BACKENDS (such as "html md").
;; Whole documents name no author unless the document does: Kalem does
;; not know the user's name.
(when (getenv "KALEM_EXPORT_FULL")
  (setq user-full-name ""))

(let ((cases (expand-file-name (car command-line-args-left)))
      (out (expand-file-name (cadr command-line-args-left)))
      (recursive (equal (nth 2 command-line-args-left) "recursive"))
      (only (getenv "KALEM_EXPORT_BACKENDS")))
  (when only
    (setq kalem-export-backends
          (seq-filter (lambda (b) (member (symbol-name (car b)) (split-string only)))
                      kalem-export-backends)))
  (make-directory out t)
  (dolist (f (if recursive
                 (directory-files-recursively cases "\\.org\\'")
               (directory-files cases t "\\.org\\'")))
    (let ((name (and recursive
                     (replace-regexp-in-string
                      "/" "__" (file-name-sans-extension (file-relative-name f cases))))))
      (kalem-export-file f out name))
    (message "exported %s" (file-name-nondirectory f))))

;;; export.el ends here
