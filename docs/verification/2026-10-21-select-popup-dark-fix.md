# 修复：设置里的原生下拉菜单白底白字

日期：2026-10-21
范围：`src/themes/tokens.css`、`src/components/controls/SettingsPopover.svelte`、`src/app/App.svelte`

## 现象

设置面板里的 `<select>`（专注模式 / 外观 / 图标 / 显示 / 查看会话）展开后，弹层是**白底**，选项文字又是接近白色，只有鼠标悬停到某一行（出现高亮）才看得见。

## 根因

1. 弹层（原生下拉列表）不继承我们在 `select` 上写的半透明背景 —— 它默认是白底；
2. 之前的写法 `option { color: var(--island-text-primary); background: var(--island-control-surface); }`：
   - `--island-text-primary` 是 `#f5f7fb`（近白）→ 白底上不可见；
   - 而 `--island-control-surface` 在石墨主题里是 `rgba(255,255,255,0.065)`（**半透明**），盖在白底上还是白，等于没有底色。

实测计算样式确认：`optionBackground = rgba(255,255,255,0.067)`，`optionColor = rgb(245,247,251)`。

## 修法

1. `tokens.css` 新增三个**不透明**的菜单 token（弹层必须不透明）：

```css
--menu-surface: #14181f;
--menu-surface-hover: #26303d;
--menu-text: #f5f7fb;
```

并用 `--island-menu-*` 映射到岛的作用域。

2. `SettingsPopover.svelte`：

```css
select { color-scheme: dark; }
select option { color-scheme: dark; color: var(--island-menu-text); background-color: var(--island-menu-surface); }
select option:checked { background-color: var(--island-menu-surface-hover); }
```

3. `App.svelte` 补文档级声明：`:global(html) { color-scheme: dark; }` ——
   让弹层、滚动条等原生控件按深色渲染，而不是跟随系统浅色。

## 验证（实机截图对照）

| | 修复前 | 修复后 |
| --- | --- | --- |
| 外观 → 纯黑 | 白底 + 白字，不可见 | 深底 + 浅字，清晰可读 |
| 专注模式 → 聚焦错误 / 静音通知 | 同上 | 同上 |

截图：`mac-island-analysis/select-popup-before.png`（白底）、`mac-island-analysis/popup-theme.png`、`mac-island-analysis/popup-zoom.png`（修复后）。

## 回归门禁

`check` 0/0 · `unit` 123 · `frontend` 26 · `browser` 86 · `visual` 14。

## 已知小瑕疵

当前选中项在弹层里用的是系统高亮色（浅灰底 + 深字），和白底时代的观感不同，但可读；非选中项已统一为深底浅字。原生弹层的选中高亮由浏览器/系统绘制，CSS 只能影响到这么多。
