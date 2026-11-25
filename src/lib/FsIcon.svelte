<script>
  // Icon ổ / thư mục / tệp dùng chung (trang Ổ cứng, Dọn dẹp…): cùng glyph, cùng màu (--folder cho thư mục).
  // kind: disk | folder | file | files | recycle. link = junction/symlink (mờ), denied = không có quyền đọc.
  import Glyph from './Glyph.svelte';
  let { kind = 'folder', link = false, denied = false, size = 14 } = $props();
  const GLYPH = { disk: 'disk', folder: 'folder', file: 'document', files: 'document', recycle: 'delete' };
</script>

<span class="fs-ico {kind}" class:link class:denied><Glyph name={GLYPH[kind] || 'folder'} {size} /></span>
