;;; dump.el --- Dump org-element parse trees as JSON  -*- lexical-binding: t; -*-

;; Part of Kalem's differential testing (design document section 5.7).
;; Licensed under MIT OR Apache-2.0.

;;; Commentary:

;; Usage:
;;
;;   emacs -Q --batch -l tests/emacs/dump.el INPUT.org [OUTPUT.json]
;;   emacs -Q --batch -l tests/emacs/dump.el --batch-dir OUTDIR INPUT.org...
;;
;; With --batch-dir, the dump of the Nth input is OUTDIR/N.json (N as five
;; digits, from 00000).
;;
;; Every element and object is written as a JSON object:
;;
;;   {"type": "headline", "begin": 0, "end": 42, "cb": 12, "ce": 42,
;;    "pa": 0, "pb": 1, "props": {...}, "children": [...],
;;    "secondary": {"title": [...]}}
;;
;; Positions are 0-based byte offsets into the UTF-8 file, so they compare
;; directly with Rust byte offsets.  "cb"/"ce" are contents-begin and
;; contents-end, "pa" is post-affiliated, "pb" is post-blank.  Plain text is
;; not written; only elements and objects are.

;;; Code:

(require 'org)
(require 'org-element)
(require 'org-inlinetask)
(require 'org-attach)
(require 'json)

(defconst kalem-dump-skip-props
  '(:begin :end :contents-begin :contents-end :post-blank :post-affiliated
    :parent :buffer :deferred :cached :org-element--cache-sync-key
    :robust-begin :robust-end :mode :granularity :secondary :structure
    :title :tag :prefix :suffix :value-begin :value-end :standard-properties)
  "Properties that are not written into \"props\".")

(defconst kalem-dump-props
  '((headline :level :todo-keyword :todo-type :priority :tags :raw-value
              :commentedp :archivedp :footnote-section-p :pre-blank)
    (inlinetask :level :todo-keyword :todo-type :priority :tags :raw-value)
    (keyword :key :value)
    (babel-call :call :inside-header :arguments :end-header :value)
    (src-block :language :switches :parameters :value)
    (example-block :switches :value)
    (export-block :type :value)
    (special-block :type)
    (comment-block :value)
    (verse-block)
    (dynamic-block :block-name :arguments)
    (drawer :drawer-name)
    (node-property :key :value)
    (plain-list :type)
    (item :bullet :checkbox :counter :pre-blank)
    (table :type :tblfm :value)
    (table-row :type)
    (clock :status :duration)
    (fixed-width :value)
    (comment :value)
    (diary-sexp :value)
    (latex-environment :value)
    (footnote-definition :label :pre-blank)
    (paragraph)
    (planning)
    (link :type :path :raw-link :format :search-option :application)
    (timestamp :type :raw-value :range-type :repeater-type :repeater-value
               :repeater-unit :warning-type :warning-value :warning-unit)
    (entity :name :use-brackets-p)
    (latex-fragment :value)
    (export-snippet :back-end :value)
    (footnote-reference :label :type)
    (inline-babel-call :call :inside-header :arguments :end-header)
    (inline-src-block :language :parameters :value)
    (macro :key :args)
    (statistics-cookie :value)
    (target :value)
    (radio-target :value)
    (code :value)
    (verbatim :value)
    (subscript :use-brackets-p)
    (superscript :use-brackets-p)
    (citation :style)
    (citation-reference :key))
  "Type-specific properties written into \"props\".")

(defconst kalem-dump-affiliated
  '(:name :caption :header :plot :results :attr_html :attr_latex :attr_org
    :attr_odt :attr_ascii :attr_md :attr_beamer :attr_texinfo)
  "Affiliated keyword properties written into \"props\" when present.")

(defvar kalem-dump--dos nil
  "Non-nil when the current file has DOS line endings.")

(defvar kalem-dump--bom 0
  "Length in bytes of the byte order mark dropped from the current file.")

(defun kalem-dump--byte (pos)
  "Return the 0-based byte offset in the file of buffer position POS, or nil.
With DOS line endings, every line feed before POS stood for two bytes."
  (and pos
       (+ (1- (position-bytes pos))
          kalem-dump--bom
          (if kalem-dump--dos (1- (line-number-at-pos pos t)) 0))))

(defun kalem-dump--value (value)
  "Convert VALUE to something `json-encode' understands."
  (cond
   ((null value) :json-null)
   ((eq value t) t)
   ((stringp value) (substring-no-properties value))
   ((numberp value) value)
   ((symbolp value) (symbol-name value))
   ((and (consp value) (not (proper-list-p value)))
    ;; Dual keyword values are (VALUE . SECONDARY).
    (vector (kalem-dump--value (car value)) (kalem-dump--value (cdr value))))
   ((and (listp value) (org-element-type value))
    (kalem-dump--secondary (list value)))
   ((and (listp value) (seq-every-p #'stringp value))
    (vconcat (mapcar #'substring-no-properties value)))
   ((listp value)
    ;; Affiliated keywords can be lists of lists of secondary strings or
    ;; strings.  Serialize them with their text only; positions of objects
    ;; inside affiliated values are compared through "secondary".
    (vconcat (mapcar #'kalem-dump--value value)))
   (t (format "%S" value))))

(defun kalem-dump--secondary (objects)
  "Dump the element and object nodes in OBJECTS, skipping plain strings."
  (vconcat
   (delq nil (mapcar (lambda (o) (and (org-element-type o)
                                      (not (eq (org-element-type o) 'plain-text))
                                      (kalem-dump--node o)))
                     objects))))

(defun kalem-dump--props (node type)
  "Collect the properties of NODE of TYPE."
  (let (props)
    (dolist (key (cdr (assq type kalem-dump-props)))
      (let ((v (org-element-property key node)))
        (unless (and (null v) (memq key '(:switches :parameters :priority
                                          :todo-keyword :todo-type :checkbox
                                          :counter :search-option :application
                                          :repeater-type :repeater-value
                                          :repeater-unit :warning-type
                                          :warning-value :warning-unit
                                          :inside-header :end-header :arguments
                                          :label :style :tblfm :value)))
          (push (cons (substring (symbol-name key) 1) (kalem-dump--value v)) props))))
    (when (memq type org-element-all-elements)
      (dolist (key kalem-dump-affiliated)
        (let ((v (org-element-property key node)))
          (when v
            (push (cons (substring (symbol-name key) 1) (kalem-dump--value v)) props)))))
    (nreverse props)))

(defun kalem-dump--node (node)
  "Return an alist describing NODE and its descendants."
  (let* ((type (org-element-type node))
         (children (kalem-dump--secondary (org-element-contents node)))
         (secondary
          (delq nil
                (mapcar (lambda (key)
                          (let ((v (org-element-property key node)))
                            (and v (cons (substring (symbol-name key) 1)
                                         (kalem-dump--secondary v)))))
                        (cdr (assq type org-element-secondary-value-alist)))))
         (planning
          (and (eq type 'planning)
               (delq nil
                     (mapcar (lambda (key)
                               (let ((v (org-element-property key node)))
                                 (and v (cons (substring (symbol-name key) 1)
                                              (kalem-dump--secondary (list v))))))
                             '(:scheduled :deadline :closed)))))
         (clock-value
          (and (eq type 'clock)
               (let ((v (org-element-property :value node)))
                 (and v (list (cons "value" (kalem-dump--secondary (list v)))))))))
    `(("type" . ,(symbol-name type))
      ("begin" . ,(kalem-dump--byte (org-element-property :begin node)))
      ("end" . ,(kalem-dump--byte (org-element-property :end node)))
      ("cb" . ,(or (kalem-dump--byte (org-element-property :contents-begin node)) :json-null))
      ("ce" . ,(or (kalem-dump--byte (org-element-property :contents-end node)) :json-null))
      ("pa" . ,(or (kalem-dump--byte (org-element-property :post-affiliated node)) :json-null))
      ("pb" . ,(or (org-element-property :post-blank node) 0))
      ("props" . ,(or (kalem-dump--props node type) :json-empty-object))
      ,@(and (> (length children) 0) `(("children" . ,children)))
      ,@(let ((sec (append secondary planning clock-value)))
          (and sec `(("secondary" . ,sec)))))))

(defun kalem-dump-file (input)
  "Parse INPUT with org-element and return the dump as a JSON string."
  (with-temp-buffer
    ;; Decode like Emacs normally does: UTF-8, with DOS line endings
    ;; converted.  Positions are mapped back to file offsets below.
    (let ((coding-system-for-read 'utf-8-auto))
      (insert-file-contents input))
    (setq kalem-dump--dos (eq 1 (coding-system-eol-type last-coding-system-used)))
    ;; A UTF-8 byte order mark is dropped when decoding, as when visiting.
    (setq kalem-dump--bom
          (if (string-match-p "with-signature" (symbol-name last-coding-system-used)) 3 0))
    (let ((default-directory (file-name-directory (expand-file-name input))))
      (let ((original (buffer-string)))
        ;; Broken in-buffer settings (for example an invalid #+MACRO) make
        ;; the mode setup signal; parsing still works.
        (condition-case nil (delay-mode-hooks (org-mode)) (error nil))
        ;; Startup options such as "#+STARTUP: align" modify the buffer.
        ;; Restore the file's text so both parsers see the same input.
        (unless (equal original (buffer-string))
          (let ((inhibit-read-only t))
            (erase-buffer)
            (insert original))))
      (let* ((tree (org-element-parse-buffer 'object))
             (json-encoding-pretty-print nil)
             (json-null :json-null)
             (doc `(("org-version" . ,(org-version))
                    ("dos" . ,(if kalem-dump--dos t :json-false))
                    ("link-types" . ,(vconcat (org-link-types)))
                    ("file" . ,(file-name-nondirectory input))
                    ("size" . ,(kalem-dump--byte (point-max)))
                    ("children" . ,(kalem-dump--secondary (org-element-contents tree))))))
        (kalem-dump--encode doc)))))

(defun kalem-dump--encode (value)
  "Encode VALUE as JSON, mapping :json-null and :json-empty-object."
  (cond
   ((eq value :json-null) "null")
   ((eq value :json-empty-object) "{}")
   ((eq value t) "true")
   ((eq value :json-false) "false")
   ((and (consp value) (consp (car value)) (stringp (caar value)))
    (concat "{"
            (mapconcat (lambda (pair)
                         (concat (json-encode-string (car pair)) ":"
                                 (kalem-dump--encode (cdr pair))))
                       value ",")
            "}"))
   ((vectorp value)
    (concat "[" (mapconcat #'kalem-dump--encode value ",") "]"))
   ((null value) "null")
   (t (json-encode value))))

(defun kalem-dump-main ()
  "Entry point for batch use."
  (let ((args command-line-args-left) outdir)
    (setq command-line-args-left nil)
    (when (equal (car args) "--batch-dir")
      (setq outdir (cadr args) args (cddr args)))
    (cond
     (outdir
      (make-directory outdir t)
      ;; Outputs are numbered by position (00000.json, ...): corpora hold
      ;; many files with the same name, such as index.org.
      (seq-do-indexed
       (lambda (input i)
        (let ((out (expand-file-name (format "%05d.json" i) outdir)))
          (condition-case err
              (with-temp-file out
                (set-buffer-file-coding-system 'utf-8-unix)
                (insert (kalem-dump-file input)))
            (error (message "FAILED %s: %S" input err)))))
       args))
     ((null args)
      (message "Usage: emacs -Q --batch -l dump.el INPUT.org [OUTPUT.json]")
      (kill-emacs 2))
     (t
      (let ((json (kalem-dump-file (car args))))
        (if (cadr args)
            (with-temp-file (cadr args)
              (set-buffer-file-coding-system 'utf-8-unix)
              (insert json))
          (princ json)
          (terpri)))))))

;; `model.el' loads this file for its helpers and sets this variable.
(defvar kalem-dump-no-main nil)

(when (and noninteractive (not kalem-dump-no-main))
  (kalem-dump-main))

;;; dump.el ends here
