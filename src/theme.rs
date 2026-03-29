use base64::{Engine as _, engine::general_purpose::STANDARD};

pub fn app_css() -> String {
    let geist_pixel = STANDARD.encode(include_bytes!("../assets/GeistPixel-Square.woff2"));
    let departure_mono = STANDARD.encode(include_bytes!("../assets/DepartureMono-Regular.woff2"));

    format!(
        r#"
@font-face {{
  font-family: "Geist Pixel";
  src: url("data:font/woff2;base64,{geist_pixel}") format("woff2");
  font-style: normal;
  font-weight: 400;
  font-display: swap;
}}

@font-face {{
  font-family: "Departure Mono";
  src: url("data:font/woff2;base64,{departure_mono}") format("woff2");
  font-style: normal;
  font-weight: 400;
  font-display: swap;
}}

:root {{
  color-scheme: dark;
  font-family: "Geist Pixel", monospace;
  background: #060606;
  color: rgba(255, 255, 255, 0.94);
}}

* {{
  box-sizing: border-box;
}}

html, body {{
  min-height: 100%;
  margin: 0;
  background: transparent;
  overflow: hidden;
}}

body {{
  min-height: 100vh;
  font-family: "Geist Pixel", monospace;
}}

button, input, textarea {{
  font: inherit;
}}

button {{
  border: 0;
  background: transparent;
  color: inherit;
}}

textarea {{
  resize: none;
}}

.app-shell {{
  min-height: 100vh;
  background: #060606;
  overflow: hidden;
}}

.frame {{
  width: 100%;
  height: 100vh;
  margin: 0 auto;
  padding: 0 16px 0;
  position: relative;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}}

.topbar {{
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  z-index: 20;
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 8px 16px 10px;
  margin-inline: 0;
  background:
    linear-gradient(180deg, rgba(10, 10, 10, 0.66) 0%, rgba(10, 10, 10, 0.34) 100%);
  backdrop-filter: blur(24px) saturate(155%);
  -webkit-backdrop-filter: blur(24px) saturate(155%);
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.28);
}}

.titlebar-drag {{
  flex: 1;
  min-width: 24px;
  height: 40px;
  border-radius: 18px;
  cursor: grab;
}}

.titlebar-drag:active {{
  cursor: grabbing;
}}

.nav-cluster,
.window-cluster {{
  display: flex;
  align-items: center;
  gap: 6px;
}}

.nav-home-anchor {{
  position: relative;
  width: 40px;
  height: 40px;
  flex: 0 0 40px;
}}

.side-new-chat-anchor {{
  position: absolute;
  left: 16px;
  top: 50%;
  z-index: 16;
  width: 40px;
  height: 40px;
  transform: translateY(-50%);
  display: inline-flex;
  align-items: center;
  justify-content: center;
}}

.side-export-anchor {{
  position: absolute;
  left: 16px;
  top: calc(50% + 46px);
  z-index: 16;
  width: 40px;
  height: 40px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}}

.side-drive-anchor {{
  position: absolute;
  left: 16px;
  top: calc(50% + 92px);
  z-index: 16;
  width: 40px;
  height: 40px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}}

.side-new-chat-button {{
  width: 40px;
  height: 40px;
  border-radius: 18px;
}}

.side-export-button {{
  width: 40px;
  height: 40px;
  border-radius: 18px;
  font-size: 14px;
}}

.side-drive-button {{
  width: 40px;
  height: 40px;
  border-radius: 18px;
  font-size: 15px;
}}

.icon-button,
.circle-button {{
  font-family: "Departure Mono", monospace;
}}

.icon-button {{
  width: 40px;
  height: 40px;
  border-radius: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  outline: none;
  color: rgba(255, 255, 255, 0.42);
  transition: background 140ms ease, color 140ms ease;
  cursor: pointer;
}}

.icon-button:hover,
.icon-button.active {{
  background: rgba(255, 255, 255, 0.08);
  color: rgba(255, 255, 255, 0.94);
}}

.icon-button.icon-button-destructive:hover,
.icon-button.icon-button-destructive.active {{
  background: rgba(255, 116, 132, 0.12);
  color: #ff8fa1;
}}

.nav-inbox-button {{
  position: absolute;
  top: 46px;
  left: 0;
  width: 40px;
  height: 40px;
  border-radius: 18px;
  font-size: 14px;
  color: rgba(255, 255, 255, 0.56);
  background: rgba(255, 255, 255, 0.04);
}}

.nav-inbox-button:hover {{
  background: rgba(255, 255, 255, 0.08);
  color: rgba(255, 255, 255, 0.88);
}}

.nav-agents-button {{
  position: absolute;
  top: 92px;
  left: 0;
  width: 40px;
  height: 40px;
  border-radius: 18px;
  font-size: 14px;
  color: rgba(255, 255, 255, 0.56);
  background: rgba(255, 255, 255, 0.04);
}}

.nav-agents-button:hover {{
  background: rgba(255, 255, 255, 0.08);
  color: rgba(255, 255, 255, 0.88);
}}

.nav-badge {{
  position: absolute;
  top: 4px;
  right: 4px;
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  border-radius: 999px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 255, 255, 0.92);
  color: rgba(0, 0, 0, 0.92);
  font-family: "Departure Mono", monospace;
  font-size: 9px;
  line-height: 1;
}}

.nav-running {{
  animation: nav-pulse 1.2s ease-in-out infinite;
  color: #ff7de4;
  text-shadow:
    0 0 10px rgba(255, 125, 228, 0.72),
    0 0 24px rgba(184, 110, 255, 0.34);
  filter: drop-shadow(0 0 12px rgba(214, 112, 255, 0.34));
}}

.nav-running.active {{
  background: rgba(214, 112, 255, 0.14);
}}

@keyframes nav-pulse {{
  0% {{
    transform: scale(1);
    box-shadow: 0 0 0 rgba(214, 112, 255, 0);
  }}
  50% {{
    transform: scale(1.16);
    box-shadow:
      0 0 26px rgba(255, 125, 228, 0.32),
      0 0 40px rgba(184, 110, 255, 0.18),
      inset 0 0 18px rgba(214, 112, 255, 0.1);
  }}
  100% {{
    transform: scale(1);
    box-shadow: 0 0 0 rgba(214, 112, 255, 0);
  }}
}}

.window-button:hover {{
  background: rgba(255, 255, 255, 0.08);
}}

.window-button.icon-button-destructive:hover {{
  background: rgba(255, 116, 132, 0.12);
}}

.settings-actions {{
  justify-content: flex-end;
}}

.settings-notice {{
  margin-top: 0;
}}

.content {{
  flex: 1;
  padding-top: 0;
  padding-bottom: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}}

.chat-screen {{
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  gap: 0;
}}

.chat-main {{
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}}

.messages {{
  display: flex;
  flex-direction: column;
  gap: 16px;
  margin: 0 auto;
  width: min(980px, 100%);
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-top: 68px;
  padding-bottom: 168px;
  scrollbar-width: none;
}}

.messages::-webkit-scrollbar {{
  width: 0;
  height: 0;
}}

.jump-bottom-button {{
  position: absolute;
  left: 50%;
  bottom: 132px;
  z-index: 16;
  width: 34px;
  height: 34px;
  border-radius: 999px;
  border: 0;
  outline: none;
  font-family: "Departure Mono", monospace;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: rgba(12, 12, 12, 0.82);
  color: rgba(255, 255, 255, 0.76);
  backdrop-filter: blur(18px) saturate(145%);
  -webkit-backdrop-filter: blur(18px) saturate(145%);
  opacity: 0;
  transform: translateX(-50%);
  pointer-events: none;
}}

.jump-bottom-button[data-visible="true"] {{
  opacity: 1;
  transform: translateX(-50%);
  pointer-events: auto;
}}

.jump-bottom-button:hover {{
  background: rgba(22, 22, 22, 0.9);
  color: rgba(255, 255, 255, 0.94);
}}

.chat-runtime-dialog {{
  position: absolute;
  inset: 72px 0 152px;
  z-index: 17;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  pointer-events: none;
}}

.chat-runtime-dialog-card {{
  width: min(420px, calc(100% - 32px));
  padding: 18px 20px;
  border-radius: 18px;
  background: rgba(10, 10, 10, 0.8);
  border: 1px solid rgba(255, 255, 255, 0.1);
  box-shadow: 0 18px 60px rgba(0, 0, 0, 0.28);
  backdrop-filter: blur(30px) saturate(165%);
  -webkit-backdrop-filter: blur(30px) saturate(165%);
}}

.chat-runtime-dialog-title {{
  margin-bottom: 8px;
  font-family: "Departure Mono", monospace;
  font-size: 11px;
  line-height: 1;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: rgba(255, 255, 255, 0.5);
}}

.chat-runtime-dialog-copy {{
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
  line-height: 1.6;
  text-align: center;
}}

.message {{
  max-width: min(78%, 100%);
  padding: 0;
  line-height: 1.7;
}}

.message-title {{
  margin-bottom: 4px;
  color: rgba(255, 255, 255, 0.34);
  font-size: 10px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}}

.message-text {{
  word-break: break-word;
}}

.message-text > :first-child {{
  margin-top: 0;
}}

.message-text > :last-child {{
  margin-bottom: 0;
}}

.message-text p,
.message-text ul,
.message-text ol,
.message-text pre,
.message-text blockquote,
.message-text table,
.message-text hr {{
  margin: 0 0 12px;
}}

.message-text h1,
.message-text h2,
.message-text h3,
.message-text h4,
.message-text h5,
.message-text h6 {{
  margin: 0 0 10px;
  color: rgba(255, 255, 255, 0.96);
  line-height: 1.28;
}}

.message-text h1,
.message-text h2 {{
  font-size: 16px;
}}

.message-text h3,
.message-text h4,
.message-text h5,
.message-text h6 {{
  font-size: 14px;
}}

.message-text ul,
.message-text ol {{
  padding-left: 20px;
}}

.message-text li + li {{
  margin-top: 4px;
}}

.message-text strong {{
  color: rgba(255, 255, 255, 0.97);
}}

.message-text em {{
  color: rgba(255, 255, 255, 0.88);
}}

.message-text code {{
  font-family: "Departure Mono", monospace;
  font-size: 0.95em;
  padding: 0 4px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.08);
  color: rgba(255, 255, 255, 0.92);
}}

.message-text pre {{
  overflow-x: auto;
  padding: 12px 14px;
  border-radius: 14px;
  background: rgba(255, 255, 255, 0.05);
}}

.message-text pre code {{
  padding: 0;
  background: transparent;
}}

.message-text blockquote {{
  margin-left: 0;
  padding-left: 12px;
  border-left: 1px solid rgba(255, 255, 255, 0.18);
  color: rgba(255, 255, 255, 0.72);
}}

.message-text a {{
  color: #a7d4ff;
  text-decoration: none;
}}

.message-text a:hover {{
  color: #c4e2ff;
  text-decoration: underline;
}}

.message-text table {{
  border-collapse: collapse;
  width: 100%;
}}

.message-text th,
.message-text td {{
  padding: 8px 10px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  text-align: left;
}}

.message-text hr {{
  border: 0;
  border-top: 1px solid rgba(255, 255, 255, 0.1);
}}

.message-agent .message-title {{
  color: rgba(var(--agent-rgb), 0.9);
}}

.message-agent.message-assistant {{
  color: rgba(var(--agent-rgb), 0.92);
}}

.message-agent.message-reasoning {{
  color: rgba(var(--agent-rgb), 0.72);
}}

.message-agent.message-activity {{
  color: rgba(var(--agent-rgb), 0.82);
}}

.message-agent.message-activity .message-text {{
  color: rgba(var(--agent-rgb), 0.84);
}}

.message-agent.message-command {{
  color: rgba(var(--agent-rgb), 0.82);
}}

.message-agent .command-toggle,
.message-agent .command-output {{
  color: rgba(var(--agent-rgb), 0.82);
}}

.message-agent .command-caret {{
  color: rgba(var(--agent-rgb), 0.58);
}}

.message-user {{
  align-self: flex-end;
  text-align: right;
  color: rgba(255, 255, 255, 0.96);
}}

.message-assistant {{
  align-self: flex-start;
  color: rgba(255, 255, 255, 0.92);
}}

.message-reasoning {{
  align-self: flex-start;
  color: rgba(255, 255, 255, 0.62);
  font-size: 12px;
}}

.message-activity {{
  align-self: flex-start;
  color: rgba(255, 255, 255, 0.78);
  font-size: 12px;
}}

.message-activity .message-title {{
  color: rgba(255, 255, 255, 0.52);
}}

.message-activity .message-text {{
  color: rgba(255, 255, 255, 0.82);
}}

.message-command {{
  align-self: flex-start;
  color: rgba(255, 255, 255, 0.74);
}}

.message-command .message-title,
.message-command .message-text,
.command-toggle,
.command-output {{
  font-family: "Departure Mono", monospace;
}}

.command-toggle {{
  width: 100%;
  padding: 0;
  display: inline-flex;
  align-items: center;
  gap: 10px;
  color: rgba(255, 255, 255, 0.74);
  font-size: 13px;
  cursor: default;
  text-align: left;
}}

.command-toggle-expandable {{
  cursor: pointer;
}}

.command-toggle:disabled {{
  opacity: 1;
}}

.command-toggle-expandable:hover {{
  color: rgba(255, 255, 255, 0.92);
}}

.command-text {{
  white-space: pre-wrap;
  word-break: break-word;
}}

.command-caret {{
  flex: 0 0 auto;
  color: rgba(255, 255, 255, 0.4);
  transition: transform 140ms ease, color 140ms ease;
}}

.command-toggle-expandable:hover .command-caret {{
  color: rgba(255, 255, 255, 0.8);
}}

.command-caret.expanded {{
  transform: rotate(180deg);
}}

.command-output {{
  margin-top: 8px;
  padding-left: 18px;
  color: rgba(255, 255, 255, 0.58);
  font-size: 13px;
  white-space: pre-wrap;
}}

.message-status {{
  align-self: flex-start;
  color: rgba(255, 152, 152, 0.92);
}}

.message-loader {{
  display: inline-flex;
  align-items: center;
  gap: 0;
  min-height: 24px;
  color: rgba(255, 255, 255, 0.7);
  font-family: 'Departure Mono', monospace;
  font-size: 18px;
  line-height: 1;
}}

.message-loader-dot {{
  display: inline-block;
  width: 0.68em;
  text-align: center;
  opacity: 0.16;
  animation: message-loader-pulse 1.05s steps(1, end) infinite;
}}

.message-loader-dot:nth-child(2) {{
  animation-delay: 0.2s;
}}

.message-loader-dot:nth-child(3) {{
  animation-delay: 0.4s;
}}

@keyframes message-loader-pulse {{
  0%, 100% {{
    opacity: 0.16;
  }}

  33% {{
    opacity: 0.38;
  }}

  66% {{
    opacity: 0.9;
  }}
}}

.composer-wrap {{
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 18;
  display: flex;
  justify-content: center;
  padding: 20px 16px 0;
  background:
    linear-gradient(180deg, rgba(6, 6, 6, 0) 0%, rgba(6, 6, 6, 0.24) 16%, rgba(6, 6, 6, 0.62) 100%);
  pointer-events: none;
}}

.composer {{
  position: relative;
  isolation: isolate;
  overflow: visible;
  width: min(980px, 100%);
  border-radius: 24px 24px 0 0;
  border-top: 1px solid rgba(255, 255, 255, 0.14);
  border-left: 1px solid rgba(255, 255, 255, 0.14);
  border-right: 1px solid rgba(255, 255, 255, 0.14);
  border-bottom: 0;
  background: rgba(10, 10, 10, 0.72);
  backdrop-filter: blur(44px) saturate(175%);
  -webkit-backdrop-filter: blur(44px) saturate(175%);
  padding: 12px 16px 14px 16px;
  pointer-events: auto;
}}

.composer::before {{
  content: "";
  position: absolute;
  inset: 1px 1px 0 1px;
  z-index: 0;
  border-radius: 23px 23px 0 0;
  background:
    linear-gradient(180deg, rgba(18, 18, 18, 0.82) 0%, rgba(10, 10, 10, 0.76) 100%);
  backdrop-filter: blur(52px) saturate(185%);
  -webkit-backdrop-filter: blur(52px) saturate(185%);
}}

.composer > * {{
  position: relative;
  z-index: 1;
}}

.composer-attachments {{
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 10px;
}}

.attachment-chip {{
  max-width: min(320px, 100%);
  min-height: 28px;
  padding: 0 8px 0 10px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.06);
  color: rgba(255, 255, 255, 0.82);
}}

.attachment-name {{
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
}}

.attachment-remove {{
  width: 18px;
  height: 18px;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  color: rgba(255, 255, 255, 0.5);
  background: transparent;
  outline: none;
}}

.attachment-remove:hover {{
  background: rgba(255, 116, 132, 0.12);
  color: #ff8fa1;
}}

.composer textarea {{
  width: 100%;
  min-height: 42px;
  max-height: 180px;
  height: 42px;
  border: 0;
  outline: none;
  background: transparent;
  color: rgba(255, 255, 255, 0.94);
  padding: 8px 0;
  display: block;
  overflow: hidden;
  font-size: 15px;
  line-height: 1.75;
}}

.composer-row {{
  margin-top: 8px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}}

.composer-controls {{
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}}

.composer-actions {{
  display: flex;
  align-items: center;
  gap: 8px;
}}

.composer-new-chat-anchor {{
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}}

.new-chat-button {{
  color: rgba(255, 255, 255, 0.52);
}}

.new-chat-button:disabled {{
  opacity: 0.62;
}}

.new-chat-button:hover:enabled,
.new-chat-button-armed {{
  background: rgba(255, 255, 255, 0.08);
  color: rgba(255, 255, 255, 0.92);
}}

.new-chat-dialog {{
  position: absolute;
  right: -2px;
  bottom: calc(100% + 12px);
  min-width: 188px;
  padding: 10px 12px;
  border-radius: 0;
  background: rgba(12, 12, 12, 0.96);
  color: rgba(255, 255, 255, 0.86);
  box-shadow:
    0 0 0 1px rgba(255, 255, 255, 0.1),
    0 10px 28px rgba(0, 0, 0, 0.32);
  image-rendering: pixelated;
}}

.side-new-chat-dialog {{
  right: auto;
  left: calc(100% + 14px);
  bottom: 50%;
  transform: translateY(50%);
}}

.new-chat-dialog::before {{
  content: "";
  position: absolute;
  inset: -4px;
  border: 2px solid rgba(255, 255, 255, 0.08);
  pointer-events: none;
}}

.new-chat-dialog-label {{
  font-family: "Departure Mono", monospace;
  font-size: 11px;
  line-height: 1.15;
  color: rgba(255, 255, 255, 0.94);
}}

.new-chat-dialog-copy {{
  margin-top: 6px;
  font-size: 11px;
  line-height: 1.35;
  color: rgba(255, 255, 255, 0.66);
}}

.new-chat-dialog-tail {{
  position: absolute;
  right: 14px;
  bottom: -8px;
  width: 12px;
  height: 12px;
  background: rgba(12, 12, 12, 0.96);
  transform: rotate(45deg);
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.08);
}}

.side-new-chat-dialog .new-chat-dialog-tail {{
  right: auto;
  left: -8px;
  bottom: calc(50% - 6px);
}}

.context-meter {{
  position: absolute;
  inset: 0 0 0 0;
  margin-top: 0;
  padding: 0;
  pointer-events: none;
  z-index: 3;
}}

.context-meter-svg {{
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: visible;
  shape-rendering: geometricPrecision;
}}

.context-meter-progress {{
  fill: none;
  stroke-width: 1.15;
  vector-effect: non-scaling-stroke;
}}

.context-meter-progress {{
  stroke: rgba(255, 255, 255, 0.96);
  stroke-linecap: butt;
  stroke-linejoin: round;
  filter: none;
}}

.context-meter-top-label {{
  position: absolute;
  top: 0;
  left: 50%;
  transform: translate(-50%, -52%);
  padding: 0 14px;
  background: rgba(12, 12, 12, 0.96);
  border-radius: 999px;
  z-index: 4;
}}

.context-meter-label {{
  display: inline-block;
  padding: 0;
  font-family: "Departure Mono", monospace;
  font-size: 11px;
  line-height: 1.1;
  letter-spacing: 0.01em;
  color: rgba(255, 255, 255, 0.8);
  white-space: nowrap;
  background: transparent;
}}

.model-cycle,
.effort-cycle {{
  min-width: 0;
  min-height: 30px;
  border-radius: 8px;
  color: rgba(255, 255, 255, 0.82);
  border: 0;
  outline: none;
}}

.model-cycle {{
  padding: 0;
  display: inline-flex;
  align-items: center;
  gap: 0;
  min-height: 30px;
  padding: 0 4px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.05);
  cursor: pointer;
}}

.model-arrow {{
  width: 14px;
  height: 22px;
  border-radius: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: rgba(255, 255, 255, 0.42);
  border: 0;
  outline: none;
  transition: background 140ms ease, color 140ms ease;
  font-size: 12px;
  cursor: pointer;
}}

.model-arrow:hover {{
  background: rgba(255, 255, 255, 0.06);
  color: rgba(255, 255, 255, 0.82);
}}

.model-arrow:disabled {{
  opacity: 0.32;
  cursor: default;
}}

.model-arrow-hidden {{
  opacity: 0;
  pointer-events: none;
}}

.model-current {{
  width: 84px;
  padding: 0 6px;
  text-align: center;
}}

.model-title {{
  font-size: 13px;
  line-height: 1;
  letter-spacing: 0.015em;
  color: rgba(255, 255, 255, 0.9);
  white-space: nowrap;
}}

.mode-toggle {{
  min-width: 0;
  min-height: 25px;
  padding: 0 2px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  outline: none;
  border-radius: 0;
  background: transparent;
  color: rgba(255, 255, 255, 0.46);
  font-size: 13px;
  letter-spacing: 0.015em;
  transition: color 140ms ease;
  cursor: pointer;
}}

.mode-toggle:hover {{
  color: rgba(255, 255, 255, 0.78);
}}

.mode-toggle:disabled {{
  color: rgba(255, 255, 255, 0.22);
  cursor: default;
}}

.mode-toggle:disabled:hover {{
  color: rgba(255, 255, 255, 0.22);
}}

.mode-toggle-active.mode-toggle-fast {{
  color: #ffb8ec;
}}

.mode-toggle-active.mode-toggle-context {{
  color: #8fe0ff;
}}

.effort-cycle {{
  padding: 0;
  display: inline-flex;
  align-items: center;
  gap: 0;
  background: transparent;
  box-shadow: none;
  min-height: 24px;
}}

.effort-arrow {{
  width: 15px;
  height: 23px;
  border-radius: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: rgba(255, 255, 255, 0.42);
  border: 0;
  outline: none;
  transition: background 140ms ease, color 140ms ease;
  font-size: 13px;
  cursor: pointer;
}}

.effort-arrow:hover {{
  background: rgba(255, 255, 255, 0.06);
  color: rgba(255, 255, 255, 0.82);
}}

.effort-arrow:disabled {{
  opacity: 0.32;
  cursor: default;
}}

.effort-arrow-hidden {{
  opacity: 0;
  pointer-events: none;
}}

.effort-current {{
  width: 52px;
  padding: 0 1px;
  text-align: center;
}}

.effort-title {{
  font-size: 13px;
  line-height: 1;
  letter-spacing: 0.015em;
}}

.effort-title-minimal {{
  color: #f0d8ff;
}}

.effort-title-none {{
  color: #f5e7ff;
}}

.effort-title-low {{
  color: #e5b8ff;
}}

.effort-title-medium {{
  color: #d489ff;
}}

.effort-title-high {{
  color: #b754f3;
}}

.effort-title-xhigh {{
  color: #8e2ed0;
}}

.circle-button {{
  width: 36px;
  height: 36px;
  border-radius: 999px;
  border: 0;
  background: transparent;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  outline: none;
  transition: background 140ms ease, color 140ms ease, border-color 140ms ease;
  color: rgba(255, 255, 255, 0.4);
}}

.circle-button:hover {{
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.94);
}}

.circle-button:disabled {{
  opacity: 0.38;
  cursor: default;
}}

.circle-button:disabled:hover {{
  background: transparent;
  color: rgba(255, 255, 255, 0.4);
}}

.send-button {{
  background: white;
  color: black;
}}

.send-button:hover {{
  background: rgba(255, 255, 255, 0.92);
  color: black;
}}

.send-button:disabled:hover {{
  background: transparent;
  color: rgba(255, 255, 255, 0.4);
}}

.accounts-screen {{
  width: min(1100px, 100%);
  margin: 0 auto;
  padding-top: 84px;
}}

.inbox-screen {{
  width: min(1100px, 100%);
  margin: 0 auto;
  padding-top: 84px;
}}

.agents-screen {{
  width: min(1100px, 100%);
  margin: 0 auto;
  padding-top: 84px;
}}

.canvas-screen {{
  width: calc(100% + 32px);
  margin-inline: -16px;
  padding-top: 84px;
  min-height: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
}}

.models-screen {{
  width: min(1100px, 100%);
  margin: 0 auto;
  padding-top: 84px;
}}

.settings-screen {{
  width: min(1100px, 100%);
  margin: 0 auto;
  padding-top: 84px;
}}

.canvas-header,
.inbox-header,
.agents-header,
.accounts-header,
.models-header,
.settings-page-header {{
  display: grid;
  grid-template-columns: 18px minmax(0, 1fr) minmax(220px, 320px) 88px;
  align-items: center;
  column-gap: 16px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.12);
  padding: 0 18px 18px;
}}

.accounts-header {{
  position: relative;
}}

.canvas-header {{
  width: min(1100px, 100%);
  margin: 0 auto;
  grid-template-columns: 18px minmax(0, 1fr);
  position: relative;
}}

.canvas-header::after {{
  content: "";
  position: absolute;
  left: 18px;
  right: 18px;
  bottom: 0;
  height: 1px;
  background: rgba(255, 255, 255, 0.12);
}}

.canvas-count,
.inbox-count,
.agents-count,
.accounts-count,
.models-count,
.settings-page-count {{
  grid-column: 1 / 4;
  font-size: 34px;
  line-height: 1;
}}

.inbox-list {{
  margin-top: 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}}

.agents-list {{
  margin-top: 18px;
}}

.inbox-card,
.inbox-empty,
.agents-empty,
.agent-tree-row {{
  border-radius: 28px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.04);
  padding: 18px 20px;
}}

.inbox-card {{
  display: flex;
  flex-direction: column;
  gap: 14px;
}}

.inbox-card-top {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}}

.inbox-source {{
  color: rgba(255, 255, 255, 0.58);
  font-size: 12px;
}}

.inbox-question,
.inbox-empty,
.agents-empty {{
  color: rgba(255, 255, 255, 0.92);
  font-size: 15px;
  line-height: 1.7;
}}

.inbox-empty,
.agents-empty {{
  color: rgba(255, 255, 255, 0.54);
}}

.agent-tree-root,
.agent-tree-children {{
  list-style: none;
  margin: 0;
  padding: 0;
}}

.agent-tree-root {{
  display: flex;
  flex-direction: column;
  gap: 12px;
}}

.agent-tree-item {{
  position: relative;
}}

.agent-tree-children {{
  margin-top: 10px;
  margin-left: 22px;
  padding-left: 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}}

.agent-tree-children::before {{
  content: "";
  position: absolute;
  left: 9px;
  top: 58px;
  bottom: 14px;
  width: 1px;
  background: rgba(255, 255, 255, 0.1);
}}

.agent-tree-children > .agent-tree-item::before {{
  content: "";
  position: absolute;
  left: -18px;
  top: 26px;
  width: 18px;
  height: 1px;
  background: rgba(255, 255, 255, 0.1);
}}

.agent-tree-row {{
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding-left: 18px;
}}

.agent-tree-row::before {{
  content: "";
  position: absolute;
  left: 20px;
  top: 20px;
  width: 7px;
  height: 7px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.3);
  transform: translateX(-18px);
}}

.agent-tree-title {{
  color: rgba(255, 255, 255, 0.94);
  font-size: 15px;
  line-height: 1.2;
}}

.agent-tree-status,
.agent-tree-meta {{
  color: rgba(255, 255, 255, 0.58);
  font-size: 12px;
  line-height: 1.4;
}}

.agent-tree-detail {{
  color: rgba(255, 255, 255, 0.78);
  font-size: 13px;
  line-height: 1.55;
}}

.agent-tree-node-queued::before {{
  background: #d9c486;
}}

.agent-tree-node-running::before {{
  background: #84f5b7;
  box-shadow: 0 0 14px rgba(132, 245, 183, 0.2);
}}

.agent-tree-node-waiting::before {{
  background: #9ed8ff;
}}

.agent-tree-node-complete::before {{
  background: rgba(255, 255, 255, 0.44);
}}

.agent-tree-node-failed::before,
.agent-tree-node-interrupted::before {{
  background: #ff8fa1;
}}

.inbox-actions {{
  display: flex;
  justify-content: flex-end;
}}

.inbox-actions button {{
  padding: 10px 14px;
  border-radius: 16px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.03);
  cursor: pointer;
}}

.inbox-actions button:hover {{
  background: rgba(255, 255, 255, 0.08);
}}

.inbox-actions button:disabled {{
  opacity: 0.42;
  cursor: default;
}}

.inbox-actions button:disabled:hover {{
  background: rgba(255, 255, 255, 0.03);
}}

.inbox-dismiss {{
  flex: 0 0 40px;
}}

.inbox-answer-input {{
  width: 100%;
  min-height: 58px;
  border-radius: 18px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.02);
  color: rgba(255, 255, 255, 0.94);
  padding: 13px 15px;
  outline: none;
}}

.inbox-answer-input:focus {{
  border-color: rgba(255, 255, 255, 0.18);
}}

.settings-panel {{
  margin-top: 18px;
  border-radius: 28px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.04);
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}}

.settings-divider-label {{
  margin-top: 4px;
  color: rgba(255, 255, 255, 0.94);
  font-size: 16px;
  line-height: 1.2;
}}

.settings-inline-note {{
  margin: 0;
}}

.settings-mode-row {{
  display: inline-flex;
  align-items: center;
  gap: 16px;
}}

.settings-mode-toggle {{
  padding: 0;
  color: rgba(255, 255, 255, 0.54);
}}

.settings-mode-toggle.mode-toggle-active {{
  color: rgba(255, 255, 255, 0.96);
}}

.settings-mode-toggle.mode-toggle-active:first-child {{
  color: #8fe0ff;
}}

.settings-mode-toggle.mode-toggle-active:last-child {{
  color: #ffb8ec;
}}

.canvas-shell {{
  position: relative;
  flex: 1;
  min-height: 0;
  margin-top: 0;
  border-left: 1px solid rgba(255, 255, 255, 0.12);
  border-right: 1px solid rgba(255, 255, 255, 0.12);
  border-bottom: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 0 0 26px 26px;
  background: linear-gradient(180deg, rgba(14, 14, 16, 0.96), rgba(8, 8, 9, 0.98));
  overflow: hidden;
}}

.swarm-canvas-viewport {{
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  cursor: grab;
}}

.swarm-canvas-viewport[data-dragging="true"] {{
  cursor: grabbing;
}}

.swarm-canvas-grid,
.swarm-canvas-stage {{
  position: absolute;
  inset: 0;
}}

.swarm-canvas-grid {{
  background-size: 56px 48px;
  background-position: center center;
  background-image:
    linear-gradient(60deg, rgba(255, 255, 255, 0.028) 1px, transparent 1px),
    linear-gradient(-60deg, rgba(255, 255, 255, 0.028) 1px, transparent 1px),
    linear-gradient(0deg, rgba(255, 255, 255, 0.02) 1px, transparent 1px);
  mask-image: radial-gradient(circle at center, black 72%, transparent 100%);
}}

.swarm-canvas-stage {{
  width: 1400px;
  height: 1200px;
  transform-origin: 0 0;
}}

.swarm-edge {{
  position: absolute;
  height: 1px;
  background: linear-gradient(90deg, rgba(255, 255, 255, 0.14), rgba(255, 255, 255, 0.05));
  transform-origin: 0 50%;
  pointer-events: none;
}}

.swarm-node {{
  position: absolute;
  padding: 16px 16px 14px;
  border-radius: 24px;
  text-align: left;
  color: rgba(255, 255, 255, 0.92);
  background: rgba(255, 255, 255, 0.035);
  border: 1px solid rgba(255, 255, 255, 0.09);
  backdrop-filter: blur(18px) saturate(150%);
  -webkit-backdrop-filter: blur(18px) saturate(150%);
  box-shadow: 0 16px 36px rgba(0, 0, 0, 0.28);
  cursor: pointer;
}}

.swarm-node:hover,
.swarm-node-selected {{
  border-color: rgba(255, 255, 255, 0.22);
  box-shadow: 0 18px 44px rgba(0, 0, 0, 0.34);
}}

.swarm-node-brain {{
  background: linear-gradient(180deg, rgba(255, 144, 220, 0.12), rgba(255, 255, 255, 0.04));
}}

.swarm-node-turn {{
  background: linear-gradient(180deg, rgba(108, 174, 255, 0.12), rgba(255, 255, 255, 0.035));
}}

.swarm-node-agent {{
  background: linear-gradient(180deg, rgba(255, 197, 110, 0.12), rgba(255, 255, 255, 0.035));
}}

.swarm-node-activity {{
  background: linear-gradient(180deg, rgba(126, 220, 255, 0.09), rgba(255, 255, 255, 0.03));
}}

.swarm-node-command {{
  background: linear-gradient(180deg, rgba(196, 150, 255, 0.1), rgba(255, 255, 255, 0.03));
}}

.swarm-node-running {{
  border-color: rgba(255, 125, 228, 0.34);
  box-shadow: 0 18px 44px rgba(0, 0, 0, 0.34);
}}

.swarm-node-waiting {{
  border-color: rgba(129, 212, 255, 0.32);
  box-shadow: 0 18px 44px rgba(0, 0, 0, 0.34);
}}

.swarm-node-queued {{
  border-color: rgba(255, 255, 255, 0.18);
}}

.swarm-node-complete {{
  border-color: rgba(140, 255, 192, 0.26);
}}

.swarm-node-failed {{
  border-color: rgba(255, 116, 132, 0.28);
}}

.swarm-node-interrupted {{
  border-color: rgba(255, 197, 116, 0.3);
}}

.swarm-node-title {{
  font-size: 13px;
  color: rgba(255, 255, 255, 0.96);
}}

.swarm-node-subtitle {{
  margin-top: 6px;
  font-size: 10px;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: rgba(255, 255, 255, 0.56);
}}

.swarm-node-detail {{
  margin-top: 10px;
  font-size: 11px;
  line-height: 1.45;
  color: rgba(255, 255, 255, 0.78);
}}

.models-table {{
  display: grid;
  grid-template-columns: minmax(170px, 1.1fr) 116px 116px 110px minmax(220px, 1.55fr) 64px 64px;
  padding-top: 0;
  padding-left: 0;
  padding-right: 0;
  column-gap: 0;
}}

.models-row {{
  display: contents;
}}

.models-row-head {{
  display: contents;
}}

.models-cell {{
  min-width: 0;
  padding: 16px 14px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
  display: flex;
  align-items: center;
}}

.models-cell + .models-cell {{
  border-left: 1px solid rgba(255, 255, 255, 0.1);
}}

.models-table > .models-cell:nth-child(7n + 1) {{
  border-left: 1px solid rgba(255, 255, 255, 0.1);
}}

.models-table > .models-cell:nth-child(7n) {{
  border-right: 1px solid rgba(255, 255, 255, 0.1);
}}

.models-head-cell {{
  padding-top: 14px;
  padding-bottom: 14px;
  color: rgba(255, 255, 255, 0.42);
  font-size: 10px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}}

.models-model {{
  color: rgba(255, 255, 255, 0.94);
  font-size: 13px;
  line-height: 1.35;
  padding-left: 18px;
}}

.models-table > .models-cell:nth-child(7n) {{
  padding-right: 18px;
}}

.models-reasoning {{
  color: rgba(255, 255, 255, 0.92);
  font-size: 13px;
  line-height: 1.45;
}}

.star-stack {{
  position: relative;
  width: 52px;
  height: 16px;
}}

.star-mark {{
  position: absolute;
  top: 0;
  left: 0;
  font-family: "Departure Mono", monospace;
  font-size: 13px;
  line-height: 1;
}}

.star-mark-intelligence {{
  color: #f4c35a;
  text-shadow: 0 0 10px rgba(244, 195, 90, 0.18);
}}

.star-mark-speed {{
  color: #9be7ff;
  text-shadow: 0 0 10px rgba(155, 231, 255, 0.16);
}}

.models-intelligence,
.models-speed,
.models-fast,
.models-context {{
  justify-content: center;
}}

.models-access {{
  justify-content: center;
}}

.access-chip {{
  min-height: 26px;
  padding: 0 10px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border-radius: 999px;
  font-size: 11px;
}}

.access-chip-api {{
  background: rgba(255, 184, 236, 0.08);
  color: #ffb8ec;
}}

.access-chip-both {{
  background: rgba(143, 224, 255, 0.08);
  color: #8fe0ff;
}}

.access-mark {{
  font-size: 12px;
  line-height: 1;
}}

.trait-icon {{
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  font-family: "Departure Mono", monospace;
  background: rgba(255, 255, 255, 0.04);
  font-size: 12px;
  line-height: 1;
}}

.trait-icon-yes {{
  color: #b6ff8a;
  background: rgba(182, 255, 138, 0.08);
}}

.trait-icon-no {{
  color: #ff7d92;
  background: rgba(255, 125, 146, 0.08);
}}

.accounts-actions {{
  grid-column: 4;
  position: absolute;
  top: 50%;
  right: 18px;
  display: flex;
  width: 88px;
  align-items: center;
  justify-self: end;
  transform: translateY(-50%);
  gap: 8px;
}}

.stats-row {{
  display: flex;
  flex-wrap: wrap;
  gap: 24px;
  padding-top: 18px;
  color: rgba(255, 255, 255, 0.52);
  font-size: 13px;
}}

.stats-row strong {{
  color: rgba(255, 255, 255, 0.94);
  margin-left: 10px;
  font-weight: 400;
}}

.accounts-list {{
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-top: 22px;
}}

.entry {{
  display: grid;
  grid-template-columns: 18px minmax(0, 1fr) minmax(220px, 320px) 40px;
  align-items: center;
  gap: 18px;
  border-radius: 20px;
  border: none;
  background: rgba(255, 255, 255, 0.03);
  padding: 16px 18px;
}}

.entry.no-rails {{
  grid-template-columns: 18px minmax(0, 1fr) 40px;
}}

.entry-dot {{
  width: 10px;
  height: 10px;
  border-radius: 999px;
  background: #b6ff8a;
  box-shadow: 0 0 18px rgba(182, 255, 138, 0.45);
  justify-self: center;
  align-self: center;
}}

.entry-copy {{
  min-width: 0;
  text-align: left;
}}

.entry-title {{
  font-size: 14px;
  line-height: 1.35;
  color: rgba(255, 255, 255, 0.94);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}}

.entry-subtitle {{
  margin-top: 4px;
  color: rgba(255, 255, 255, 0.54);
  font-size: 11px;
  line-height: 1.25;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}}

.entry-rails {{
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}}

.rail {{
  display: grid;
  grid-template-columns: minmax(0, 1fr) 40px;
  align-items: center;
  gap: 6px;
  color: rgba(255, 255, 255, 0.52);
  font-size: 11px;
}}

.rail-track {{
  display: grid;
  grid-template-columns: 88px minmax(0, 1fr);
  align-items: center;
  gap: 6px;
  min-width: 0;
}}

.rail-reset,
.rail-percent {{
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}}

.rail-reset {{
  text-align: right;
}}

.rail-percent {{
  text-align: left;
  width: 40px;
}}

.rail-bar {{
  height: 6px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.14);
  overflow: hidden;
}}

.rail-fill {{
  height: 100%;
  border-radius: inherit;
  background: rgba(255, 255, 255, 0.96);
}}

.entry-actions {{
  display: flex;
  width: 40px;
  align-items: center;
  gap: 0;
  justify-content: flex-end;
  justify-self: end;
}}

.text-button {{
  padding: 8px 10px;
  border-radius: 14px;
  color: rgba(255, 255, 255, 0.5);
  cursor: pointer;
}}

.text-button:hover {{
  background: rgba(255, 255, 255, 0.08);
  color: rgba(255, 255, 255, 0.94);
}}

.modal {{
  margin-top: 18px;
  border-radius: 28px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.04);
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}}

.toggle-row,
.input-row,
.modal-actions,
.pending-box {{
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}}

.toggle-row button,
.modal-actions button {{
  padding: 10px 14px;
  border-radius: 16px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.03);
  cursor: pointer;
}}

.toggle-row button.active,
.toggle-row button:hover,
.modal-actions button:hover {{
  background: rgba(255, 255, 255, 0.08);
}}

.field {{
  flex: 1 1 240px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}}

.field label {{
  color: rgba(255, 255, 255, 0.44);
  font-size: 12px;
}}

.field input {{
  width: 100%;
  border-radius: 18px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  background: rgba(255, 255, 255, 0.02);
  color: rgba(255, 255, 255, 0.94);
  padding: 13px 15px;
  outline: none;
}}

.field input:focus {{
  border-color: rgba(255, 255, 255, 0.2);
}}

.pending-box {{
  align-items: center;
  justify-content: space-between;
  border-top: 1px solid rgba(255, 255, 255, 0.08);
  padding-top: 14px;
}}

.notice {{
  margin-top: 14px;
  color: rgba(255, 255, 255, 0.58);
  font-size: 12px;
  line-height: 1.6;
}}

.muted {{
  margin: 0;
  color: rgba(255, 255, 255, 0.52);
  font-size: 13px;
  line-height: 1.7;
}}

@media (max-width: 900px) {{
  .topbar {{
    margin-inline: 0;
  }}

  .frame {{
    width: 100%;
    padding: 0 12px 0;
  }}

  .entry {{
    grid-template-columns: 18px minmax(0, 1fr) auto;
    align-items: start;
  }}

  .entry.no-rails {{
    grid-template-columns: 18px minmax(0, 1fr) auto;
  }}

  .entry-rails {{
    grid-column: 2 / 4;
    width: 100%;
  }}

  .entry-actions {{
    grid-column: 2 / 4;
  }}

  .rail {{
    grid-template-columns: 1fr;
    gap: 6px;
  }}

  .rail-bar {{
    order: 2;
  }}

  .rail-percent {{
    order: 3;
    text-align: left;
  }}
}}
"#
    )
}
