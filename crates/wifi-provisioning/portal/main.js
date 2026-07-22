const form = document.querySelector("#wifi-form");
const network = document.querySelector("#network");
const networkDetail = document.querySelector("#network-detail");
const manualField = document.querySelector("#manual-field");
const ssid = document.querySelector("#ssid");
const passwordField = document.querySelector("#password-field");
const password = document.querySelector("#password");
const passwordDetail = document.querySelector("#password-detail");
const refreshButton = document.querySelector("#refresh");
const revealButton = document.querySelector("#reveal");
const submitButton = document.querySelector("#submit");
const buttonLabel = submitButton.querySelector(".button-label");
const notice = document.querySelector("#notice");
const noticeTitle = document.querySelector("#notice-title");
const noticeCopy = document.querySelector("#notice-copy");

let networks = [];
let accepted = false;

function setNotice(kind, title, copy) {
  notice.hidden = false;
  notice.className = `notice ${kind}`;
  noticeTitle.textContent = title;
  noticeCopy.textContent = copy;
}

function clearNotice() {
  notice.hidden = true;
  notice.className = "notice";
}

function selectedNetwork() {
  if (network.value === "__manual__") return null;
  return networks.find((item) => item.ssid === network.value) || null;
}

function updateNetworkFields() {
  const manual = network.value === "__manual__";
  const selected = selectedNetwork();
  manualField.hidden = !manual;
  ssid.required = manual;
  password.required = Boolean(selected?.secure);
  passwordDetail.textContent = selected?.secure
    ? "This network is secured. Passwords must be 8-63 characters."
    : selected
      ? "This is an open network; no password is required."
      : "Use at least 8 characters for secured networks.";
  networkDetail.textContent = selected
    ? `${selected.rssi >= -55 ? "Excellent" : selected.rssi >= -70 ? "Good" : "Weak"} signal  ·  ${selected.secure ? "Secured" : "Open network"}`
    : manual
      ? "Enter the exact network name below."
      : "Choose a network to continue.";
  if (manual) ssid.focus();
  clearNotice();
}

async function scan() {
  refreshButton.classList.add("scanning");
  refreshButton.disabled = true;
  network.disabled = true;
  networkDetail.textContent = "Scanning nearby Wi-Fi…";
  try {
    const response = await fetch("/networks", { cache: "no-store" });
    if (!response.ok) throw new Error("scan failed");
    networks = await response.json();
    const previous = network.value;
    network.replaceChildren();
    for (const item of networks) {
      const option = document.createElement("option");
      option.value = item.ssid;
      option.textContent = `${item.ssid}  ·  ${item.rssi} dBm${item.secure ? "  ·  Secured" : "  ·  Open"}`;
      network.append(option);
    }
    const manual = document.createElement("option");
    manual.value = "__manual__";
    manual.textContent = "Other or hidden network…";
    network.append(manual);
    network.value = [...network.options].some((option) => option.value === previous)
      ? previous
      : networks[0]?.ssid || "__manual__";
    network.disabled = false;
    updateNetworkFields();
  } catch {
    networks = [];
    network.replaceChildren(new Option("Other or hidden network…", "__manual__"));
    network.disabled = false;
    network.value = "__manual__";
    updateNetworkFields();
    setNotice("error", "Couldn’t refresh networks", "You can still enter the network name manually.");
  } finally {
    refreshButton.classList.remove("scanning");
    refreshButton.disabled = false;
  }
}

function setBusy(busy) {
  submitButton.disabled = busy;
  network.disabled = busy;
  ssid.disabled = busy;
  password.disabled = busy;
  refreshButton.disabled = busy;
  buttonLabel.textContent = busy ? "Connecting…" : "Connect device";
}

async function watchStatus() {
  for (let attempt = 0; attempt < 16; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 900));
    try {
      const response = await fetch("/status", { cache: "no-store" });
      if (!response.ok) continue;
      const status = await response.json();
      if (status.state === "connected") {
        setNotice("success", "You’re connected", "The device is online. You can close this window.");
        buttonLabel.textContent = "Connected";
        return;
      }
      if (status.state === "failed") {
        accepted = false;
        setBusy(false);
        setNotice("error", "Couldn’t join that network", status.message || "Check the password and try again.");
        password.focus();
        return;
      }
    } catch {
      if (accepted && attempt > 1) {
        setNotice("success", "Setup sent", "If the setup network disappeared, your device connected successfully.");
        buttonLabel.textContent = "Setup complete";
        return;
      }
    }
  }
  setNotice("success", "Setup sent", "The device is finishing the connection. You can close this window.");
  buttonLabel.textContent = "Setup complete";
}

network.addEventListener("change", updateNetworkFields);
refreshButton.addEventListener("click", scan);
revealButton.addEventListener("click", () => {
  const showing = password.type === "text";
  password.type = showing ? "password" : "text";
  passwordField.classList.toggle("revealed", !showing);
  revealButton.setAttribute("aria-label", showing ? "Show password" : "Hide password");
});

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  clearNotice();
  const name = network.value === "__manual__" ? ssid.value.trim() : network.value;
  if (!name) {
    setNotice("error", "Network name required", "Enter the name of the Wi-Fi network.");
    ssid.focus();
    return;
  }
  if (password.required && (password.value.length < 8 || password.value.length > 63)) {
    setNotice("error", "Check the password", "Secured Wi-Fi passwords must be 8-63 characters.");
    password.focus();
    return;
  }
  setBusy(true);
  setNotice("", "Joining your network", "Keep this page open while the device connects.");
  try {
    const body = new URLSearchParams({ ssid: name, password: password.value });
    const response = await fetch("/connect", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body,
    });
    const result = await response.json();
    if (!response.ok) throw new Error(result.message || "The device rejected those details.");
    accepted = true;
    await watchStatus();
  } catch (error) {
    setBusy(false);
    setNotice("error", "Couldn’t start setup", error.message || "Reconnect to the device and try again.");
  }
});

scan();
