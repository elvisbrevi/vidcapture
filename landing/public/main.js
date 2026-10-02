// Copy-to-clipboard and the viewfinder timecode. The page is complete without
// this file: commands stay selectable and the timecode just reads 00:00:00:00.
(function () {
  'use strict';

  document.querySelectorAll('.copy').forEach(function (btn) {
    btn.addEventListener('click', function () {
      var text = btn.getAttribute('data-copy') || '';
      var label = btn.textContent;
      var flash = function (msg) {
        btn.textContent = msg;
        setTimeout(function () { btn.textContent = label; }, 1200);
      };

      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(
          function () { flash('copied'); },
          function () { flash('⌘C'); }
        );
        return;
      }

      // No async clipboard: select the command so it can be copied by hand.
      var code = btn.parentNode.querySelector('code');
      if (code && window.getSelection) {
        var range = document.createRange();
        range.selectNodeContents(code);
        var sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
      }
    });
  });

  var timecode = document.getElementById('timecode');
  if (!timecode || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;

  var start = performance.now();
  var pad = function (n) { return n < 10 ? '0' + n : String(n); };

  // HH:MM:SS:FF at 30 fps, like a camera HUD. Ticks only while visible.
  function tick(now) {
    var ms = now - start;
    var s = Math.floor(ms / 1000);
    var frames = Math.floor((ms % 1000) / (1000 / 30));
    timecode.textContent =
      pad(Math.floor(s / 3600)) + ':' + pad(Math.floor(s / 60) % 60) + ':' + pad(s % 60) + ':' + pad(frames);
    requestAnimationFrame(tick);
  }

  requestAnimationFrame(tick);
})();
