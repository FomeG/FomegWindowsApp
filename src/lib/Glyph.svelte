<script>
  // Biểu tượng từ font icon của Windows (Segoe Fluent Icons / Segoe MDL2 Assets), như RustWTM.
  // Không tốn tài nguyên tải (font có sẵn trong Windows 10/11). Thiếu font (trình duyệt ngoài
  // Windows) thì html có lớp `no-icon-font` và glyph bị ẩn (xem app.css).
  import { GLYPHS } from './glyphs.js';
  let { name, size = 16, class: cls = '', title } = $props();
  const ch = $derived(GLYPHS[name] ? String.fromCodePoint(GLYPHS[name]) : '');
</script>

<span class="glyph {cls}" style="font-size:{size}px" aria-hidden={title ? undefined : 'true'} {title}>{ch}</span>
