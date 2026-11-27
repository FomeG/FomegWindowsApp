<script module>
  // Icon nét của RustWTM (crates/wtm-gui/src/navigation.rs::paint), vẽ lại bằng SVG với ĐÚNG toạ độ gốc:
  // hộp 19×19, nét 1,6 px, đầu nét vuông (egui). Hình chữ nhật của egui vẽ nét "bên trong" nên lùi vào 0,8 px.
  // Dùng cho menu bên trái và các chỗ đại diện cho một trang (tiêu đề ngăn chi tiết…), để mọi nơi cùng một bộ icon.
  const S = 0.8; // nửa nét
  const box = (x, y, w, h) => `M${x + S} ${y + S}h${w - 2 * S}v${h - 2 * S}h${-(w - 2 * S)}Z`;
  const line = (...pts) => 'M' + pts.map(([x, y]) => `${x} ${y}`).join('L');
  const circle = (x, y, r) => `M${x - r} ${y}a${r} ${r} 0 1 0 ${2 * r} 0a${r} ${r} 0 1 0 ${-2 * r} 0`;
  const ellipse = (x, y, rx, ry) => `M${x - rx} ${y}a${rx} ${ry} 0 1 0 ${2 * rx} 0a${rx} ${ry} 0 1 0 ${-2 * rx} 0`;

  const gear = () => {
    let d = circle(9.5, 9.5, 2.5) + circle(9.5, 9.5, 5.5);
    for (let i = 0; i < 8; i++) {
      const a = (i * Math.PI) / 4;
      const c = Math.cos(a), s = Math.sin(a);
      d += line([9.5 + 5.5 * c, 9.5 + 5.5 * s], [9.5 + 8 * c, 9.5 + 8 * s]);
    }
    return d;
  };
  const power = () => {
    const pts = [];
    for (let i = 0; i <= 16; i++) {
      const a = ((-55 + (i * 290) / 16) * Math.PI) / 180;
      pts.push([9.5 + 7 * Math.sin(a), 10.5 - 7 * Math.cos(a)]);
    }
    return line(...pts) + line([9.5, 1.5], [9.5, 9.5]);
  };

  export const NAV_ICONS = {
    search: circle(8, 8, 5.5) + line([12, 12], [18, 18]),
    overview: [[2, 2], [11, 2], [2, 11], [11, 11]].map(([x, y]) => box(x, y, 6, 6)).join(''),
    processes: box(2, 2, 15, 15) + [6, 10, 14].map((y) => line([5, y], [14, y])).join(''),
    performance: line([1, 10], [5, 10], [8, 3], [11, 16], [14, 8], [18, 8]),
    disk: box(2, 5, 15, 10) + line([5, 10], [11, 10]) + circle(14, 10, 0.8),
    cleanup: line([12, 9], [17, 3], [15.5, 1.8], [10.5, 8]) + line([7.5, 7.5], [13.5, 11.5], [10, 17], [2, 14], [7.5, 7.5]) + line([6, 15.5], [8.5, 12.5]),
    history: circle(9.5, 9.5, 7) + line([9.5, 5], [9.5, 10], [13, 12]),
    shield: line([9.5, 1.5], [17, 4], [16, 11], [13.5, 15], [9.5, 18], [5.5, 15], [3, 11], [2, 4], [9.5, 1.5]) + line([6, 9], [8.5, 11.5], [13, 7]),
    events: line([3, 14], [5, 11], [5, 7], [7, 3], [12, 3], [14, 7], [14, 11], [16, 14], [3, 14]) + line([7.5, 17], [11.5, 17]),
    settings: gear(),
    startup: line([9.5, 17], [9.5, 2], [3.5, 8]) + line([9.5, 2], [15.5, 8]) + line([3, 17], [16, 17]),
    services: box(2, 2, 15, 6) + box(2, 11, 15, 6) + circle(5.5, 5, 0.5) + circle(5.5, 14, 0.5),
    users: circle(9.5, 5.5, 3.5) + line([2.5, 17], [3.5, 13], [6.5, 11], [12.5, 11], [15.5, 13], [16.5, 17]),
    network: circle(9.5, 9.5, 7.5) + line([2, 9.5], [17, 9.5]) + ellipse(9.5, 9.5, 3.5, 7.5),
    // không có trong RustWTM: vẽ cùng kiểu (ô 6×6, nét 1,6) – ba ô + một ô xoay như biểu tượng Ứng dụng của Windows
    apps: box(2, 2, 6, 6) + box(2, 11, 6, 6) + box(11, 11, 6, 6) + line([14, 1.5], [17.5, 5], [14, 8.5], [10.5, 5], [14, 1.5]),
    ports: box(4, 6, 11, 7) + line([7, 2], [7, 6]) + line([12, 2], [12, 6]) + line([9.5, 13], [9.5, 18]),
    // không có trong RustWTM: "{=}" – biến và giá trị (Biến môi trường), cùng nét 1,6
    env: line([6, 2], [4.5, 2], [4.5, 8], [2.5, 9.5], [4.5, 11], [4.5, 17], [6, 17]) + line([13, 2], [14.5, 2], [14.5, 8], [16.5, 9.5], [14.5, 11], [14.5, 17], [13, 17]) + line([7.5, 7.5], [11.5, 7.5]) + line([7.5, 11.5], [11.5, 11.5]),
    about: circle(9.5, 9.5, 7.5) + circle(9.5, 5.5, 0.5) + line([9.5, 9], [9.5, 14]),
    power: power(),
    stop: line([6, 2], [13, 2], [17, 6], [17, 13], [13, 17], [6, 17], [2, 13], [2, 6], [6, 2]) + line([6.5, 6.5], [12.5, 12.5]) + line([6.5, 12.5], [12.5, 6.5]),
  };
</script>

<script>
  let { name, size = 19 } = $props();
</script>

<svg class="navicon" width={size} height={size} viewBox="0 0 19 19" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="miter" aria-hidden="true">
  <path d={NAV_ICONS[name] || ''} />
</svg>
