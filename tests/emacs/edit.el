;;; edit.el --- Run Org editing commands for Kalem's differential tests  -*- lexical-binding: t; -*-

;; Part of Kalem's differential testing (design document sections 6.2 and 16).
;; Licensed under MIT OR Apache-2.0.

;;; Commentary:

;; Usage:
;;
;;   emacs -Q --batch -l tests/emacs/edit.el CASES.json OUTPUT.json
;;
;; CASES.json is an array of objects:
;;
;;   {"name": "...", "text": "...", "point": BYTE, "mark": BYTE or null,
;;    "form": "(org-do-demote)", "note": TEXT (optional)}
;;
;; Each case runs in a fresh Org buffer holding TEXT, with point (and the
;; region, when MARK is given) at the byte offsets, and evaluates FORM.
;; The output is an array of {"name", "text", "point", "error"}, with the
;; point as a byte offset.  The clock is fixed at 2026-09-28 Mon 10:00 so
;; that commands that insert timestamps are deterministic.

;;; Code:

(require 'org)
(require 'org-inlinetask)
(require 'org-list)
(require 'org-table)
(require 'json)

(defconst kalem-edit-now (encode-time 0 0 10 28 9 2026)
  "The fixed time commands see.")

(defun kalem-edit--pos (byte)
  "Buffer position of 0-based BYTE offset."
  (byte-to-position (1+ byte)))

(defun kalem-edit--byte (pos)
  "0-based byte offset of buffer position POS."
  (1- (position-bytes pos)))

(defun kalem-edit--fixed-time (orig &optional time &rest args)
  "Call ORIG with TIME, or the fixed time when TIME is nil."
  (apply orig (or time kalem-edit-now) args))

(defun kalem-edit--fixed-times (orig a b)
  "Call ORIG with A and B, the fixed time standing for nil."
  (funcall orig (or a kalem-edit-now) (or b kalem-edit-now)))

;; `string-collate-lessp' follows the collation of the system's locale,
;; and on macOS ignores IGNORE-CASE.  Kalem compares code points, and
;; ignores case when asked (as Emacs does on GNU/Linux): the reference
;; results do too, whatever the machine.
;; (Redefined rather than advised: callers check its arity.)
(fset 'string-collate-lessp
      (lambda (a b &optional _locale ignore-case)
        (if ignore-case
            (string-lessp (downcase a) (downcase b))
          (string-lessp a b))))

;; Primitives that read the clock themselves when given nil.
(defun kalem-edit--fixed-format (orig format &optional time &rest args)
  "Call ORIG with FORMAT and TIME, the fixed time standing for nil."
  (apply orig format (or time kalem-edit-now) args))

(advice-add 'format-time-string :around #'kalem-edit--fixed-format)
(advice-add 'float-time :around #'kalem-edit--fixed-time)
(advice-add 'time-less-p :around #'kalem-edit--fixed-times)
(advice-add 'time-subtract :around #'kalem-edit--fixed-times)
(advice-add 'time-to-days :around #'kalem-edit--fixed-time)

(defun kalem-edit-case (case)
  "Run CASE, an alist from the JSON input, and return the result alist.
The form runs as a command: `post-command-hook' runs after it, which is
where Org writes log entries.  When CASE has a \"note\", it is typed into
the note buffer Org opens and stored."
  (let ((name (alist-get 'name case))
        (text (alist-get 'text case))
        (point (alist-get 'point case))
        (mark (alist-get 'mark case))
        (note (alist-get 'note case))
        (form (car (read-from-string (alist-get 'form case))))
        buf err)
    (with-temp-buffer
      (setq buf (current-buffer))
      (insert text)
      (let ((inhibit-message t)
            (org-inhibit-startup t))
        (delay-mode-hooks (org-mode)))
      ;; As in a window: fontified, so that link markup is invisible, which
      ;; column counts and table widths take into account.
      (let ((inhibit-message t)) (font-lock-ensure))
      (goto-char (kalem-edit--pos point))
      (when (and mark (not (eq mark :null)))
        (push-mark (kalem-edit--pos mark) t t))
      (cl-letf (((symbol-function 'current-time) (lambda () kalem-edit-now))
                ((symbol-function 'y-or-n-p) (lambda (&rest _) nil))
                ((symbol-function 'yes-or-no-p) (lambda (&rest _) nil))
                ((symbol-function 'read-string) (lambda (&rest _) (error "Input needed")))
                ((symbol-function 'read-char-exclusive) (lambda (&rest _) (error "Input needed")))
                ;; No windows: switching buffers is enough.
                ((symbol-function 'pop-to-buffer)
                 (lambda (b &rest _) (set-buffer (get-buffer-create b))))
                ((symbol-function 'current-window-configuration) #'ignore)
                ((symbol-function 'set-window-configuration) #'ignore))
        ;; A note set up by a failed command must not leak into the next
        ;; case, as the command loop's `post-command-hook' ensures.
        (setq org-log-setup nil)
        ;; A table edited by the previous case must not look aligned.
        (setq org-table-may-need-update t)
        (let ((this-command 'kalem-edit-command)
              (last-command nil))
          (condition-case e
              (let ((inhibit-message t)
                    (transient-mark-mode t))
                (eval form t)
                (run-hooks 'post-command-hook)
                (when (get-buffer "*Org Note*")
                  (with-current-buffer "*Org Note*"
                    (goto-char (point-max))
                    ;; Without a note, the user cancels it.
                    (let ((org-note-abort (not (stringp note))))
                      (when (stringp note) (insert note))
                      (org-store-log-note)))))
            (error (setq err (error-message-string e)))))
        (set-buffer buf)
        (remove-hook 'post-command-hook 'org-add-log-note)
        (when (get-buffer "*Org Note*")
          (kill-buffer "*Org Note*")))
      `(("name" . ,name)
        ("text" . ,(buffer-substring-no-properties (point-min) (point-max)))
        ("point" . ,(kalem-edit--byte (point)))
        ("error" . ,(or err :json-null))))))

(defun kalem-edit-main ()
  "Batch entry point."
  (let* ((args command-line-args-left)
         (cases (let ((json-object-type 'alist) (json-array-type 'list)
                      (json-null :null) (json-false nil))
                  (json-read-file (car args))))
         (results (mapcar #'kalem-edit-case cases)))
    (setq command-line-args-left nil)
    (with-temp-file (cadr args)
      (set-buffer-file-coding-system 'utf-8-unix)
      (let ((json-encoding-pretty-print t) (json-null :json-null))
        (insert (json-encode (vconcat results)))
        (insert "\n")))))

(when noninteractive
  (kalem-edit-main))

;;; edit.el ends here
