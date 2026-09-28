;;; model.el --- Dump Org's document model as JSON  -*- lexical-binding: t; -*-

;; Part of Kalem's differential testing (design document sections 5.7 and 6.1).
;; Licensed under MIT OR Apache-2.0.

;;; Commentary:

;; Usage:
;;
;;   emacs -Q --batch -l tests/emacs/model.el INPUT.org [OUTPUT.json]
;;   emacs -Q --batch -l tests/emacs/model.el --batch-dir OUTDIR INPUT.org...
;;
;; With --batch-dir, the model of the Nth input is OUTDIR/N.json (five
;; digits, from 00000).
;;
;; Where dump.el writes the syntax tree, this file writes what Org computes
;; from it: TODO sequences, file tags, keyword properties, and for every
;; headline its tags (local and inherited), category, properties (standard,
;; special, and looked up with and without inheritance), plus the entries
;; that match the tags/property match strings given in the file itself with
;; "#+KALEM_MATCH:" keywords.  Positions are byte offsets, as in dump.el.

;;; Code:

(defvar kalem-dump-no-main t)
(load (expand-file-name "dump.el" (file-name-directory (or load-file-name buffer-file-name))) nil t)

(defconst kalem-model-probe-properties
  '("CATEGORY" "ARCHIVE" "COLUMNS" "LOGGING" "ID" "CUSTOM_ID" "EFFORT")
  "Properties looked up in every entry, besides those found in the file.")

(defconst kalem-model-default-matches
  '("{.}" "-{.}" "/!" "/{.}" "LEVEL=2" "LEVEL>2/!" "TODO=\"TODO\""
    "PRIORITY<>\"B\"" "ALLTAGS={:}" "ITEM={a}" "CATEGORY={.}"
    "SCHEDULED<>\"\"" "DEADLINE<\"<2030-01-01>\"" "TIMESTAMP_IA>\"<2000-01-01>\"")
  "Match strings tried on every file.")

(defun kalem-model--pairs (alist)
  "Encode ALIST of strings as a vector of two-element vectors."
  (vconcat (mapcar (lambda (p) (vector (kalem-dump--value (car p))
                                       (kalem-dump--value (cdr p))))
                   alist)))

(defun kalem-model--property-names ()
  "Every property name used in the buffer, upcased, in order of appearance."
  (let (names)
    (org-element-map (org-element-parse-buffer 'element) '(node-property keyword)
      (lambda (e)
        (pcase (org-element-type e)
          ('node-property
           (let ((k (upcase (org-element-property :key e))))
             (setq k (string-remove-suffix "+" k))
             (unless (member k names) (push k names))))
          ('keyword
           (when (equal (org-element-property :key e) "PROPERTY")
             (let ((v (org-element-property :value e)))
               (when (string-match "\\`\\(\\S-+\\)" v)
                 (let ((k (upcase (string-remove-suffix "+" (match-string 1 v)))))
                   (unless (member k names) (push k names))))))))))
    (dolist (k kalem-model-probe-properties)
      (unless (member k names) (push k names)))
    (nreverse names)))

(defun kalem-model--headline (names)
  "Describe the headline at point, looking up NAMES."
  (let* ((el (org-element-at-point))
         (special (seq-remove (lambda (p) (member (car p) '("FILE" "CLOCKSUM" "CLOCKSUM_T")))
                              (org-entry-properties nil 'special))))
    `(("begin" . ,(kalem-dump--byte (point)))
      ("level" . ,(org-element-property :level el))
      ("todo" . ,(kalem-dump--value (org-element-property :todo-keyword el)))
      ("local-tags" . ,(vconcat (mapcar #'substring-no-properties (org-get-tags nil t))))
      ("tags" . ,(vconcat (mapcar #'substring-no-properties (org-get-tags))))
      ("category" . ,(kalem-dump--value (org-get-category)))
      ("clock" . ,(or (get-text-property (point) :org-clock-minutes) :json-null))
      ("standard" . ,(kalem-model--pairs (org-entry-properties nil 'standard)))
      ("special" . ,(kalem-model--pairs special))
      ("get" . ,(kalem-model--pairs
                 (delq nil (mapcar (lambda (k) (let ((v (org-entry-get nil k)))
                                                 (and v (cons k v))))
                                   names))))
      ("selective" . ,(kalem-model--pairs
                       (delq nil (mapcar (lambda (k) (let ((v (org-entry-get nil k 'selective)))
                                                       (and v (cons k v))))
                                         names))))
      ("allowed" . ,(vconcat
                     (delq nil (mapcar (lambda (k)
                                         (let ((v (org-property-get-allowed-values nil k)))
                                           (and v (vector k (vconcat (mapcar #'substring-no-properties v))
                                                          (if (get-text-property 0 'org-unrestricted (car v)) t :json-false)))))
                                       (append '("TODO" "PRIORITY") names)))))
      ("inherit" . ,(kalem-model--pairs
                     (delq nil (mapcar (lambda (k) (let ((v (org-entry-get nil k t)))
                                                     (and v (cons k v))))
                                       names)))))))

(defun kalem-model--cookie-list ()
  "Statistics cookies in the buffer: begin and text."
  (let (out)
    (org-element-map (org-element-parse-buffer) 'statistics-cookie
      (lambda (c)
        (push (cons (kalem-dump--byte (org-element-begin c))
                    (buffer-substring-no-properties (org-element-begin c)
                                                    (- (org-element-end c) (org-element-post-blank c))))
              out)))
    (nreverse out)))

(defun kalem-model--cookies ()
  "Every statistics cookie before and after `org-update-statistics-cookies'.
This modifies the buffer, so it runs last."
  (let ((before (kalem-model--cookie-list)))
    (goto-char (point-min))
    (let ((inhibit-message t))
      (condition-case nil (org-update-statistics-cookies t) (error nil)))
    (let ((after (kalem-model--cookie-list)))
      (vconcat
       (cl-mapcar (lambda (b a) (vector (car b) (cdr b) (cdr a)))
                  before after)))))

(defun kalem-model--footnotes ()
  "Footnote references with a label, and their definitions."
  (require 'org-footnote)
  (let (out)
    (org-element-map (org-element-parse-buffer) 'footnote-reference
      (lambda (r)
        (let ((label (org-element-property :label r)))
          (when label
            (let ((def (save-excursion (org-footnote-get-definition label))))
              (push (vector (kalem-dump--byte (org-element-begin r)) label
                            (if def (kalem-dump--byte (nth 1 def)) :json-null))
                    out))))))
    (vconcat (nreverse out))))

(defun kalem-model--links ()
  "Internal links and where `org-open-at-point' would go, as byte offsets."
  (require 'org-id)
  (let (out)
    (org-element-map (org-element-parse-buffer) 'link
      (lambda (l)
        (let ((type (org-element-property :type l))
              (path (org-element-property :path l))
              (raw (org-element-property :raw-link l)))
          (when (member type '("fuzzy" "custom-id" "coderef" "radio" "id"))
            (let ((target
                   (save-excursion
                     (save-restriction
                       (widen)
                       (condition-case nil
                           (let ((org-link-search-must-match-exact-headline t)
                                 (inhibit-message t))
                             (pcase type
                               ("radio" (org-link--search-radio-target path) (point))
                               ("id" (org-find-entry-with-id path))
                               (_ (org-link-search (if (member type '("custom-id" "coderef")) raw path))
                                  (point))))
                         (error nil))))))
              (push (vector (kalem-dump--byte (org-element-begin l)) type path
                            (if target (kalem-dump--byte target) :json-null))
                    out))))))
    (vconcat (nreverse out))))

(defun kalem-model-file (input)
  "Compute the model of INPUT and return it as a JSON string."
  (with-temp-buffer
    (let ((coding-system-for-read 'utf-8-auto))
      (insert-file-contents input))
    (setq kalem-dump--dos (eq 1 (coding-system-eol-type last-coding-system-used)))
    (setq kalem-dump--bom
          (if (string-match-p "with-signature" (symbol-name last-coding-system-used)) 3 0))
    (let ((default-directory (file-name-directory (expand-file-name input)))
          (original (buffer-string)))
      (condition-case nil (delay-mode-hooks (org-mode)) (error nil))
      (unless (equal original (buffer-string))
        (let ((inhibit-read-only t))
          (erase-buffer)
          (insert original)))
      (let* ((names (kalem-model--property-names))
             (headlines nil)
             (matches nil))
        (require 'org-clock)
        (condition-case nil (let ((inhibit-message t)) (org-clock-sum)) (error nil))
        (org-with-wide-buffer
         (goto-char (point-min))
         (while (re-search-forward org-outline-regexp-bol nil t)
           (beginning-of-line)
           ;; Headlines and inlinetasks, but not the END line of an
           ;; inlinetask.
           (let ((el (org-element-at-point)))
             (when (and (org-element-type-p el '(headline inlinetask))
                        (= (org-element-begin el) (point)))
               (push (save-excursion (kalem-model--headline names)) headlines)))
           (end-of-line)))
        (dolist (m (append
                    (cdr (assoc "KALEM_MATCH" (org-collect-keywords '("KALEM_MATCH"))))
                    kalem-model-default-matches
                    ;; Include and exclude the first tags of the file.
                    (let (ms)
                      (dolist (tag (seq-take (mapcar #'car (org-get-buffer-tags)) 3))
                        (push (concat "+" tag) ms)
                        (push (concat "-" tag "/!") ms))
                      (nreverse ms))))
          (push `(("match" . ,m)
                  ("begins" . ,(vconcat
                                (condition-case err
                                    (org-map-entries (lambda () (kalem-dump--byte (point))) m)
                                  (error (list (format "error: %S" err)))))))
                matches))
        (let* ((footnotes (kalem-model--footnotes))
               (links (kalem-model--links))
               (cookies (kalem-model--cookies))
               (doc `(("org-version" . ,(org-version))
                     ("file" . ,(file-name-nondirectory input))
                     ("todo-sets" . ,(vconcat (mapcar (lambda (s) (vconcat s)) org-todo-sets)))
                     ("todo-kwd-alist" . ,(vconcat
                                           (mapcar (lambda (e)
                                                     (vector (car e) (symbol-name (nth 1 e))
                                                             (nth 2 e) (nth 3 e) (nth 4 e)))
                                                   org-todo-kwd-alist)))
                     ("todo-keywords" . ,(vconcat org-todo-keywords-1))
                     ("done-keywords" . ,(vconcat org-done-keywords))
                     ("file-tags" . ,(vconcat (mapcar #'substring-no-properties org-file-tags)))
                     ("keyword-properties" . ,(kalem-model--pairs org-keyword-properties))
                     ("priorities" . ,(vector org-priority-highest org-priority-lowest org-priority-default))
                     ("headlines" . ,(vconcat (nreverse headlines)))
                     ("matches" . ,(vconcat (nreverse matches)))
                     ("links" . ,links)
                     ("footnotes" . ,footnotes)
                     ("cookies" . ,cookies))))
          (kalem-dump--encode doc))))))

(defun kalem-model-main ()
  "Entry point for batch use."
  (let ((args command-line-args-left))
    (setq command-line-args-left nil)
    (cond
     ((equal (car args) "--batch-dir")
      (let ((dir (cadr args)))
        (make-directory dir t)
        (seq-do-indexed
         (lambda (input i)
          (condition-case err
              (with-temp-file (expand-file-name (format "%05d.json" i) dir)
                (set-buffer-file-coding-system 'utf-8-unix)
                (insert (kalem-model-file input)))
            (error (message "FAILED %s: %S" input err))))
         (cddr args))))
     ((null args)
      (message "Usage: emacs -Q --batch -l model.el INPUT.org [OUTPUT.json]")
      (kill-emacs 2))
     (t
      (let ((json (kalem-model-file (car args))))
        (if (cadr args)
            (with-temp-file (cadr args)
              (set-buffer-file-coding-system 'utf-8-unix)
              (insert json))
          (princ json)
          (terpri)))))))

(when noninteractive
  (kalem-model-main))

;;; model.el ends here
