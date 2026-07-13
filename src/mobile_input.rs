#[derive(Clone, Copy)]
pub enum MobileInputKind {
    RoomCode = 0,
    PlayerName = 1,
}

#[cfg(target_arch = "wasm32")]
pub fn show(kind: MobileInputKind, value: &str, max_len: usize) {
    mobile_input_show(kind as u32, value, max_len as u32);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn show(_kind: MobileInputKind, _value: &str, _max_len: usize) {}

#[cfg(target_arch = "wasm32")]
pub fn value(kind: MobileInputKind) -> Option<String> {
    let value = mobile_input_value(kind as u32);
    (!value.is_empty()).then_some(value)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn value(_kind: MobileInputKind) -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
pub fn hide() {
    mobile_input_hide();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn hide() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
let mobileInput = null;
let mobileKind = -1;

function ensureInput() {
  if (mobileInput) return mobileInput;
  const wrapper = document.createElement("label");
  wrapper.id = "ghost-mobile-input-wrapper";
  wrapper.style.cssText = "position:fixed;left:max(12px,env(safe-area-inset-left));right:max(12px,env(safe-area-inset-right));bottom:max(12px,env(safe-area-inset-bottom));z-index:10000;background:#30343b;color:white;padding:8px;border-radius:8px;font:16px sans-serif;display:none";
  const caption = document.createElement("span");
  caption.id = "ghost-mobile-input-caption";
  caption.style.cssText = "display:block;margin-bottom:4px";
  const input = document.createElement("input");
  input.id = "ghost-mobile-input";
  input.type = "text";
  input.style.cssText = "box-sizing:border-box;width:100%;font-size:18px;padding:10px;border-radius:6px;border:1px solid #8ad;background:white;color:#111";
  wrapper.append(caption, input);
  document.body.append(wrapper);
  mobileInput = input;
  return input;
}

export function mobile_input_show(kind, value, maxLen) {
  if (!matchMedia("(pointer: coarse)").matches) return;
  const input = ensureInput();
  const wrapper = input.parentElement;
  wrapper.style.display = "block";
  document.getElementById("ghost-mobile-input-caption").textContent = kind === 0 ? "Private room code" : "Player name";
  input.maxLength = maxLen;
  input.autocomplete = kind === 0 ? "off" : "nickname";
  input.autocapitalize = kind === 0 ? "characters" : "words";
  if (mobileKind !== kind || document.activeElement !== input) input.value = value;
  mobileKind = kind;
}
export function mobile_input_value(kind) {
  return mobileInput && mobileKind === kind ? mobileInput.value : "";
}
export function mobile_input_hide() {
  if (!mobileInput) return;
  mobileInput.blur();
  mobileInput.parentElement.style.display = "none";
  mobileKind = -1;
}
"#)]
extern "C" {
    fn mobile_input_show(kind: u32, value: &str, max_len: u32);
    fn mobile_input_value(kind: u32) -> String;
    fn mobile_input_hide();
}
