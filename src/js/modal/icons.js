// ============== 弹窗图标 ==============

export function getModalIconSvg(type) {
  // 使用内联 SVG，避免依赖外部资源
  const common =
    'viewBox="0 0 24 24" width="22" height="22" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"';
  switch (type) {
    case "success":
      return `<svg ${common}><path d="M20 6L9 17l-5-5"></path></svg>`;
    case "warn":
      return `<svg ${common}><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path><line x1="12" y1="9" x2="12" y2="13"></line><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>`;
    case "error":
      return `<svg ${common}><circle cx="12" cy="12" r="10"></circle><line x1="15" y1="9" x2="9" y2="15"></line><line x1="9" y1="9" x2="15" y2="15"></line></svg>`;
    case "info":
    case "alert":
    case "confirm":
    default:
      return `<svg ${common}><circle cx="12" cy="12" r="10"></circle><line x1="12" y1="8" x2="12" y2="12"></line><line x1="12" y1="16" x2="12.01" y2="16"></line></svg>`;
  }
}
