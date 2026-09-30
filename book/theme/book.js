// The Kalem Book: the light or dark theme, and the search box over
// window.BOOK_INDEX (the text of every page, written by `kalem book build`).
(function () {
  var root = window.BOOK_ROOT || "";
  var html = document.documentElement;
  var themeButton = document.querySelector("header .theme");
  if (themeButton) {
    themeButton.addEventListener("click", function () {
      var dark = html.dataset.theme
        ? html.dataset.theme === "dark"
        : window.matchMedia("(prefers-color-scheme: dark)").matches;
      html.dataset.theme = dark ? "light" : "dark";
      try { localStorage.setItem("kalem-book-theme", html.dataset.theme); } catch (e) {}
    });
  }
  var input = document.querySelector("input.search");
  var results = document.querySelector(".results");
  var article = document.querySelector("article");
  if (!input || !results || !window.BOOK_INDEX) return;
  function escape(s) {
    return s.replace(/[&<>"]/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
    });
  }
  function search(q) {
    var words = q.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) {
      results.hidden = true;
      article.hidden = false;
      return;
    }
    var hits = [];
    window.BOOK_INDEX.forEach(function (p) {
      var text = p.x.toLowerCase();
      var title = p.t.toLowerCase();
      var score = 0;
      for (var i = 0; i < words.length; i++) {
        var inTitle = title.indexOf(words[i]) >= 0;
        var at = text.indexOf(words[i]);
        if (!inTitle && at < 0) return;
        score += (inTitle ? 10 : 0) + (at >= 0 ? 1 : 0);
      }
      var first = text.indexOf(words[0]);
      var from = Math.max(0, first - 60);
      var snippet = escape(p.x.slice(from, from + 180));
      words.forEach(function (w) {
        snippet = snippet.replace(new RegExp("(" + w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ")", "ig"), "<mark>$1</mark>");
      });
      hits.push({ p: p, score: score, snippet: snippet });
    });
    hits.sort(function (a, b) { return b.score - a.score; });
    results.innerHTML = hits.length
      ? hits.slice(0, 30).map(function (h) {
          return '<a href="' + root + h.p.u + '"><strong>' + escape(h.p.t) +
            '</strong><span class="snippet">' + (h.snippet ? "…" + h.snippet + "…" : "") + "</span></a>";
        }).join("")
      : "<p>Nothing found.</p>";
    results.hidden = false;
    article.hidden = true;
  }
  input.addEventListener("input", function () { search(input.value); });
  document.addEventListener("keydown", function (e) {
    if (e.key === "/" && document.activeElement !== input) {
      e.preventDefault();
      input.focus();
    } else if (e.key === "Escape" && document.activeElement === input) {
      input.value = "";
      search("");
      input.blur();
    }
  });
})();
