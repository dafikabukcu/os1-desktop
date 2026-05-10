import { lazy, Suspense, type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { AudioLines, Mic, MicOff, Pause, Settings, Terminal } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { buildVoiceInstructions } from "./voicePrompts";
import type { AssistantIdentity, PersonalityMode } from "./voiceTypes";

const BootSequence = lazy(() =>
  import("./BootSequence").then((module) => ({ default: module.BootSequence })),
);

type VoiceState = "idle" | "checking" | "starting" | "listening" | "muted";
type VoiceActivity = "idle" | "connecting" | "listening" | "user-speaking" | "assistant-speaking" | "muted";

type HermesStatus = {
  nativeAvailable: boolean;
  wslAvailable: boolean;
  workspaceReady: boolean;
  message: string;
  native?: {
    available: boolean;
    path?: string | null;
    version?: string | null;
  };
  home?: {
    path: string;
    exists: boolean;
    hasConfig: boolean;
    hasAuth: boolean;
    hasEnv: boolean;
    hasSessions: boolean;
    hasSkills: boolean;
    hasCron: boolean;
    hasKanban: boolean;
    hasStateDatabase: boolean;
    profileCount: number;
  };
  codexHome?: {
    path: string;
    exists: boolean;
  };
  wslDistros?: Array<{
    name: string;
    hermesCliAvailable: boolean;
    hermesHomeExists: boolean;
    linuxHome?: string | null;
    hermesHomePath?: string | null;
  }>;
};

type RealtimeKeyStatus = {
  configured: boolean;
  source: string;
};

type HermesProfile = {
  name: string;
  path: string;
  isDefault: boolean;
  exists: boolean;
};

type HermesProfileCatalog = {
  distro: string;
  profiles: HermesProfile[];
};

type CreateHermesProfileResult = {
  created: HermesProfile;
  catalog: HermesProfileCatalog;
  message: string;
};

type RepairHermesResult = {
  distro: string;
  message: string;
  output: string;
};

type InstallHermesResult = {
  distro: string;
  message: string;
  output: string;
};

type HermesRuntimeStatus = {
  distro: string;
  profile: string;
  hermesHome: string;
  hermesCommand?: string | null;
  version?: string | null;
  profileExists: boolean;
  hasEnv: boolean;
  hasConfig: boolean;
  hasSessions: boolean;
  hasSkills: boolean;
  hasCron: boolean;
  modelProvider?: string | null;
  modelDefault?: string | null;
  ready: boolean;
  missing: string[];
  message: string;
};

type ConfigureHermesProviderResult = {
  distro: string;
  profile: string;
  provider: string;
  model: string;
  message: string;
  output: string;
};

type ImportCodexAuthResult = {
  distro: string;
  message: string;
};

type HermesCommandResult = {
  distro: string;
  profile: string;
  output: string;
};

type ProfileCommandResult = {
  distro: string;
  profile: string;
  command: string;
  output: string;
  exitCode: number;
};

type SetupStep = "discovering" | "name" | "confirm" | "creating";
type SetupAction = "install" | "repair" | null;
type ActivePanel = "workspace" | "terminal" | null;
type ProviderChoice = "codex-subscription" | "openai-key";

type RealtimeFunctionCallItem = {
  type?: string;
  name?: string;
  call_id?: string;
  arguments?: string;
};

const LOCAL_NOISE_FLOOR = 0.032;
const LOCAL_SPEECH_START = 0.17;
const LOCAL_SPEECH_STOP = 0.075;
const LOCAL_SPEECH_FRAMES_REQUIRED = 4;
const REMOTE_SPEECH_START = 0.075;
const IDENTITY_STORAGE_KEY = "os1.assistantIdentity.v1";
const PERSONALITY_STORAGE_KEY = "os1.personalityMode.v1";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export function App() {
  const [bootFinished, setBootFinished] = useState(false);
  const [surfaceEntering, setSurfaceEntering] = useState(false);
  const [voiceState, setVoiceState] = useState<VoiceState>("idle");
  const [voiceActivity, setVoiceActivity] = useState<VoiceActivity>("idle");
  const [status, setStatus] = useState<HermesStatus | null>(null);
  const [runtimeStatus, setRuntimeStatus] = useState<HermesRuntimeStatus | null>(null);
  const [identity, setIdentity] = useState<AssistantIdentity | null>(() => loadAssistantIdentity());
  const [personalityMode, setPersonalityMode] = useState<PersonalityMode>(() => loadPersonalityMode());
  const [setupStep, setSetupStep] = useState<SetupStep>(() => (window.__TAURI_INTERNALS__ && !loadAssistantIdentity() ? "discovering" : "name"));
  const [nameInput, setNameInput] = useState("");
  const [pendingName, setPendingName] = useState("");
  const [setupError, setSetupError] = useState("");
  const [setupDistro, setSetupDistro] = useState<string | null>(null);
  const [setupAction, setSetupAction] = useState<SetupAction>(null);
  const [providerChoice, setProviderChoice] = useState<ProviderChoice>("openai-key");
  const [activePanel, setActivePanel] = useState<ActivePanel>(null);
  const [terminalInput, setTerminalInput] = useState("hermes --version");
  const [terminalOutput, setTerminalOutput] = useState("");
  const [terminalBusy, setTerminalBusy] = useState(false);
  const [doctorOutput, setDoctorOutput] = useState("");
  const [doctorBusy, setDoctorBusy] = useState(false);
  const [providerBusy, setProviderBusy] = useState(false);
  const [providerOutput, setProviderOutput] = useState("");
  const [codexImportReady, setCodexImportReady] = useState(false);
  const [, setLog] = useState<string[]>([
    "OS1 surface initialized",
    "Realtime voice bridge ready",
  ]);
  const peerRef = useRef<RTCPeerConnection | null>(null);
  const dataChannelRef = useRef<RTCDataChannel | null>(null);
  const localStreamRef = useRef<MediaStream | null>(null);
  const remoteAudioRef = useRef<HTMLAudioElement | null>(null);
  const presenceRef = useRef<HTMLDivElement | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const localAnalyserRef = useRef<AnalyserNode | null>(null);
  const remoteAnalyserRef = useRef<AnalyserNode | null>(null);
  const localSourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const remoteSourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const activityFrameRef = useRef<number | null>(null);
  const activityRef = useRef<VoiceActivity>("idle");
  const voiceStateRef = useRef<VoiceState>("idle");
  const levelsRef = useRef({ local: 0, remote: 0 });
  const localSpeechFramesRef = useRef(0);

  const isLive = voiceState === "listening";
  const isMuted = voiceState === "muted";
  const isStartingVoice = voiceState === "starting";
  const shouldShowOnboarding = !identity;
  const topStatusMessage = runtimeStatus?.message ?? status?.message ?? null;
  const runtimeRows = runtimeStatus ? makeRuntimeRows(runtimeStatus) : [];

  const finishBoot = useCallback(() => {
    setSurfaceEntering(true);
    setBootFinished(true);
    window.setTimeout(() => setSurfaceEntering(false), 1500);
  }, []);

  useEffect(() => {
    if (identity || setupStep !== "discovering") return;
    if (!window.__TAURI_INTERNALS__) {
      setSetupStep("name");
      return;
    }

    let cancelled = false;

    async function adoptExistingProfile() {
      setPendingName("OS1");
      setSetupError("");
      setSetupAction(null);
      setVoiceState("checking");
      setMotionActivity("connecting");

      try {
        const catalog = await invoke<HermesProfileCatalog>("list_hermes_profiles");
        if (cancelled) return;

        setSetupDistro(catalog.distro);
        const readyProfiles = catalog.profiles.filter((profile) => profile.exists && !profile.isDefault);
        const selectedProfile = readyProfiles.length === 1 ? readyProfiles[0] : null;

        if (!selectedProfile) {
          setSetupStep("name");
          return;
        }

        const assistantName = profileNameForDisplay(selectedProfile.name);
        setPendingName(assistantName);
        const runtime = await invoke<HermesRuntimeStatus>("check_hermes_runtime", {
          distro: catalog.distro,
          profile: selectedProfile.name,
        });
        if (cancelled) return;

        setRuntimeStatus(runtime);
        setProviderChoice(providerChoiceFromRuntime(runtime));
        if (!runtime.ready) {
          setSetupStep("name");
          appendLog(`Existing Hermes profile ${selectedProfile.name} is not ready: ${runtime.message}`);
          return;
        }

        const nextIdentity = {
          assistantName,
          profileSlug: selectedProfile.name,
          distro: catalog.distro,
          profileReady: true,
        };
        saveAssistantIdentity(nextIdentity);
        setIdentity(nextIdentity);
        appendLog(`Selected existing Hermes profile ${selectedProfile.name}`);
      } catch (error) {
        if (!cancelled) {
          appendLog(`Existing Hermes profile discovery failed: ${String(error)}`);
          setSetupStep("name");
        }
      } finally {
        if (!cancelled) {
          setVoiceState("idle");
          setMotionActivity("idle");
        }
      }
    }

    void adoptExistingProfile();
    return () => {
      cancelled = true;
    };
  }, [identity, setupStep]);

  useEffect(() => {
    if (!identity || !window.__TAURI_INTERNALS__) return;
    let cancelled = false;

    async function preloadRuntime() {
      try {
        const result = await invoke<HermesRuntimeStatus>("check_hermes_runtime", {
          distro: identity?.distro,
          profile: identity?.profileSlug,
        });
        if (!cancelled) {
          setRuntimeStatus(result);
          setProviderChoice(providerChoiceFromRuntime(result));
        }
      } catch {
        if (!cancelled) {
          setRuntimeStatus(null);
        }
      }
    }

    void preloadRuntime();
    return () => {
      cancelled = true;
    };
  }, [identity?.distro, identity?.profileSlug]);

  async function checkHermes() {
    setVoiceState("checking");
    setMotionActivity("connecting");
    try {
      if (identity && window.__TAURI_INTERNALS__) {
        const result = await invoke<HermesRuntimeStatus>("check_hermes_runtime", {
          distro: identity.distro,
          profile: identity.profileSlug,
        });
        setRuntimeStatus(result);
        setProviderChoice(providerChoiceFromRuntime(result));
        appendLog(result.message);
        if (result.version) appendLog(result.version);
        if (result.missing.length) appendLog(`Missing: ${result.missing.join(", ")}`);
      } else {
        const result = window.__TAURI_INTERNALS__
          ? await invoke<HermesStatus>("detect_hermes")
          : {
              nativeAvailable: false,
              wslAvailable: false,
              workspaceReady: false,
              message: "Preview mode; native Hermes check runs inside Tauri",
            };
        setStatus(result);
        appendLog(result.message);
        for (const detail of summarizeHermesStatus(result)) {
          appendLog(detail);
        }
      }
    } catch (error) {
      appendLog(`Hermes check failed: ${String(error)}`);
    } finally {
      setVoiceState("idle");
      setMotionActivity("idle");
    }
  }

  async function startVoice() {
    if (voiceState === "checking" || voiceState === "starting" || peerRef.current || shouldShowOnboarding) return;
    setVoiceState("starting");
    setMotionActivity("connecting");
    appendLog("Starting Realtime voice");

    try {
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

      if (!navigator.mediaDevices?.getUserMedia) {
        throw new Error("Microphone capture is not available in this webview.");
      }

      const keyStatus = window.__TAURI_INTERNALS__
        ? await invoke<RealtimeKeyStatus>("realtime_key_status")
        : { configured: false, source: "browser-preview" };
      if (!keyStatus.configured) {
        throw new Error(
          window.__TAURI_INTERNALS__
            ? "OPENAI_API_KEY is not configured for OS1."
            : "Realtime voice runs inside Tauri with OPENAI_API_KEY configured.",
        );
      }

      const peer = new RTCPeerConnection();
      peerRef.current = peer;

      const remoteAudio = document.createElement("audio");
      remoteAudio.autoplay = true;
      document.body.appendChild(remoteAudio);
      remoteAudioRef.current = remoteAudio;

      peer.ontrack = (event) => {
        remoteAudio.srcObject = event.streams[0];
        setupRemoteMeter(event.streams[0]);
        appendLog("Realtime audio connected");
      };

      peer.onconnectionstatechange = () => {
        appendLog(`Voice connection ${peer.connectionState}`);
        if (peer.connectionState === "failed" || peer.connectionState === "disconnected") {
          stopVoice(peer.connectionState);
        }
      };

      const localStream = await navigator.mediaDevices.getUserMedia({ audio: true });
      localStreamRef.current = localStream;
      setupLocalMeter(localStream);
      startActivityMeter();
      for (const track of localStream.getTracks()) {
        peer.addTrack(track, localStream);
      }

      const dataChannel = peer.createDataChannel("oai-events");
      dataChannelRef.current = dataChannel;
      dataChannel.addEventListener("open", () => {
        setVoiceState("listening");
        setMotionActivity("listening");
        appendLog("Realtime data channel open");
        dataChannel.send(
          JSON.stringify({
            type: "session.update",
            session: {
              type: "realtime",
              instructions: buildVoiceInstructions(identity, personalityMode),
              tools: buildRealtimeTools(identity),
              tool_choice: "auto",
            },
          }),
        );
      });
      dataChannel.addEventListener("message", (event) => {
        handleRealtimeEvent(event.data);
      });

      const offer = await peer.createOffer();
      await peer.setLocalDescription(offer);
      if (!offer.sdp) {
        throw new Error("Could not create local SDP offer.");
      }

      const answerSdp = await invoke<string>("create_realtime_call", { sdp: offer.sdp });
      await peer.setRemoteDescription({ type: "answer", sdp: answerSdp });
      appendLog("Realtime answer applied");
    } catch (error) {
      appendLog(`Voice start failed: ${error instanceof Error ? error.message : String(error)}`);
      stopVoice("error");
    }
  }

  function toggleMute() {
    const nextMuted = !isMuted;
    for (const track of localStreamRef.current?.getAudioTracks() ?? []) {
      track.enabled = !nextMuted;
    }
    setVoiceState(nextMuted ? "muted" : "listening");
    setMotionActivity(nextMuted ? "muted" : "listening");
    appendLog(nextMuted ? "Microphone muted" : "Microphone unmuted");
  }

  function stopVoice(reason = "stopped") {
    stopActivityMeter();
    dataChannelRef.current?.close();
    peerRef.current?.close();
    for (const track of localStreamRef.current?.getTracks() ?? []) {
      track.stop();
    }
    remoteAudioRef.current?.remove();
    dataChannelRef.current = null;
    peerRef.current = null;
    localStreamRef.current = null;
    remoteAudioRef.current = null;
    setVoiceState("idle");
    setMotionActivity("idle");
    appendLog(`Voice surface ${reason}`);
  }

  function updatePersonalityMode(nextMode: PersonalityMode) {
    setPersonalityMode(nextMode);
    savePersonalityMode(nextMode);
    appendLog(`Voice personality set to ${nextMode === "her" ? "Her" : "Assistant"}`);

    if (dataChannelRef.current?.readyState === "open") {
      dataChannelRef.current.send(
        JSON.stringify({
          type: "session.update",
          session: {
            type: "realtime",
            instructions: buildVoiceInstructions(identity, nextMode),
            tools: buildRealtimeTools(identity),
            tool_choice: "auto",
          },
        }),
      );
    }
  }

  function handleRealtimeEvent(raw: string) {
    try {
      const event = JSON.parse(raw) as {
        type?: string;
        error?: { message?: string };
        item?: RealtimeFunctionCallItem;
      };
      if (event.type === "session.created") {
        appendLog("Realtime session created");
      } else if (event.type === "error") {
        appendLog(`Realtime error: ${event.error?.message ?? "unknown"}`);
      } else if (event.type === "input_audio_buffer.speech_started") {
        if (levelsRef.current.local > LOCAL_SPEECH_START) {
          setMotionActivity("user-speaking");
        }
      } else if (event.type === "input_audio_buffer.speech_stopped") {
        setMotionActivity("listening");
      } else if (event.type === "output_audio_buffer.started" || event.type === "response.audio.delta") {
        setMotionActivity("assistant-speaking");
      } else if (event.type === "output_audio_buffer.stopped" || event.type === "response.audio.done") {
        setMotionActivity("listening");
      } else if (event.type === "response.output_item.done" && event.item?.type === "function_call") {
        void handleRealtimeFunctionCall(event.item);
      }
    } catch {
      appendLog("Realtime event received");
    }
  }

  async function handleRealtimeFunctionCall(item: RealtimeFunctionCallItem) {
    const channel = dataChannelRef.current;
    if (!channel || channel.readyState !== "open" || !identity || !item.call_id) return;

    try {
      const args = parseFunctionArguments(item.arguments);
      let output: unknown;

      if (item.name === "check_hermes_status") {
        output = await invoke<HermesRuntimeStatus>("check_hermes_runtime", {
          distro: identity.distro,
          profile: identity.profileSlug,
        });
      } else if (item.name === "ask_hermes") {
        const prompt = typeof args.prompt === "string" ? args.prompt : "";
        output = await invoke<HermesCommandResult>("ask_hermes", {
          distro: identity.distro,
          profile: identity.profileSlug,
          prompt,
        });
      } else {
        output = { error: `Unknown OS1 tool: ${item.name}` };
      }

      channel.send(
        JSON.stringify({
          type: "conversation.item.create",
          item: {
            type: "function_call_output",
            call_id: item.call_id,
            output: JSON.stringify(output),
          },
        }),
      );
      channel.send(JSON.stringify({ type: "response.create" }));
    } catch (error) {
      channel.send(
        JSON.stringify({
          type: "conversation.item.create",
          item: {
            type: "function_call_output",
            call_id: item.call_id,
            output: JSON.stringify({ error: error instanceof Error ? error.message : String(error) }),
          },
        }),
      );
      channel.send(JSON.stringify({ type: "response.create" }));
    }
  }

  function appendLog(message: string) {
    const stamp = new Date().toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
    setLog((items) => [`${stamp} ${message}`, ...items].slice(0, 7));
  }

  function summarizeHermesStatus(result: HermesStatus) {
    const details: string[] = [];
    if (result.native?.available) {
      details.push(`Native CLI: ${result.native.version || result.native.path || "found"}`);
    }
    if (result.home?.exists) {
      const signals = [
        result.home.hasConfig && "config",
        result.home.hasAuth && "auth",
        result.home.hasEnv && "env",
        result.home.hasSessions && "sessions",
        result.home.hasSkills && "skills",
        result.home.hasCron && "cron",
        result.home.hasKanban && "kanban",
      ].filter(Boolean);
      details.push(`Local home: ${signals.length ? signals.join(", ") : "folder only"}`);
    }
    const wslReady = result.wslDistros?.filter((distro) => distro.hermesCliAvailable || distro.hermesHomeExists) ?? [];
    if (wslReady.length) {
      details.push(`WSL ready: ${wslReady.map((distro) => distro.name).join(", ")}`);
    } else if (result.wslDistros?.length) {
      details.push(`WSL distros: ${result.wslDistros.map((distro) => distro.name).join(", ")}`);
    }
    if (!details.length && result.codexHome?.exists) {
      details.push("Codex home available");
    }
    return details;
  }

  function beginNameConfirmation(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const cleaned = nameInput.trim().replace(/\s+/g, " ");
    if (cleaned.length < 2) {
      setSetupError("Please choose a name with at least two characters.");
      return;
    }
    if (!slugifyProfileName(cleaned)) {
      setSetupError("Please include at least one letter or number.");
      return;
    }
    setPendingName(cleaned);
    setSetupError("");
    setSetupAction(null);
    setSetupStep("confirm");
  }

  async function confirmAssistantName() {
    const profileSlug = slugifyProfileName(pendingName);
    if (!profileSlug) {
      setSetupStep("name");
      setSetupError("Please choose a name with at least one letter or number.");
      return;
    }

    setSetupStep("creating");
    setSetupError("");
    setSetupAction(null);
    setVoiceState("checking");
    setMotionActivity("connecting");
    let selectedDistro = "pending";

    try {
      if (!window.__TAURI_INTERNALS__) {
        const previewIdentity = { assistantName: pendingName, profileSlug, distro: "preview", profileReady: false };
        saveAssistantIdentity(previewIdentity);
        setIdentity(previewIdentity);
        appendLog(`Assistant named ${pendingName}`);
        return;
      }

      const catalog = await invoke<HermesProfileCatalog>("list_hermes_profiles");
      selectedDistro = catalog.distro;
      setSetupDistro(catalog.distro);
      const existing = catalog.profiles.find((profile) => profile.name === profileSlug);
      let selectedProfile = existing;
      let selectedCatalog = catalog;
      let profileMessage = `Selected Hermes profile ${profileSlug}`;

      if (!selectedProfile) {
        const created = await invoke<CreateHermesProfileResult>("create_hermes_profile", {
          distro: catalog.distro,
          name: profileSlug,
          mode: "clone",
          cloneFrom: null,
        });
        selectedProfile = created.created;
        selectedCatalog = created.catalog;
        profileMessage = created.message;
      }

      const runtime = await invoke<HermesRuntimeStatus>("check_hermes_runtime", {
        distro: selectedCatalog.distro,
        profile: selectedProfile.name,
      });
      setRuntimeStatus(runtime);
      setProviderChoice(providerChoiceFromRuntime(runtime));
      if (!runtime.ready) {
        setSetupError(runtime.message);
        setSetupAction(runtime.missing.includes("Hermes CLI") ? "install" : "repair");
        setSetupStep("confirm");
        appendLog(`Hermes runtime not ready: ${runtime.message}`);
        return;
      }

      const provider = await invoke<ConfigureHermesProviderResult>("configure_hermes_provider", {
        distro: selectedCatalog.distro,
        profile: selectedProfile.name,
        mode: providerChoice,
      });

      const nextIdentity = {
        assistantName: pendingName,
        profileSlug: selectedProfile.name,
        distro: selectedCatalog.distro,
        profileReady: true,
      };
      saveAssistantIdentity(nextIdentity);
      setIdentity(nextIdentity);
      appendLog(`${profileMessage}`);
      appendLog(provider.message);
      appendLog(`Voice name set to ${pendingName}`);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSetupDistro(selectedDistro === "pending" ? setupDistro : selectedDistro);
      setSetupError(message);
      setSetupAction(classifySetupAction(message));
      setSetupStep("confirm");
      appendLog(`Hermes profile setup failed: ${message}`);
    } finally {
      setVoiceState("idle");
      setMotionActivity("idle");
    }
  }

  async function repairHermes() {
    setSetupStep("creating");
    setSetupError("");
    setSetupAction(null);
    setVoiceState("checking");
    setMotionActivity("connecting");

    try {
      if (!window.__TAURI_INTERNALS__) {
        setSetupError("Hermes repair runs inside the desktop app.");
        setSetupStep("confirm");
        return;
      }

      const repaired = await invoke<RepairHermesResult>("repair_hermes", {
        distro: setupDistro,
      });
      setSetupDistro(repaired.distro);
      appendLog(repaired.message);
      appendLog(repaired.output || "Hermes repair completed");
      setSetupError("Repair complete. Click Yes again to create the profile.");
      setSetupAction(null);
      setSetupStep("confirm");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSetupError(message);
      setSetupAction(classifySetupAction(message));
      setSetupStep("confirm");
      appendLog(`Hermes repair failed: ${message}`);
    } finally {
      setVoiceState("idle");
      setMotionActivity("idle");
    }
  }

  async function installHermes() {
    setSetupStep("creating");
    setSetupError("");
    setSetupAction(null);
    setVoiceState("checking");
    setMotionActivity("connecting");

    try {
      if (!window.__TAURI_INTERNALS__) {
        setSetupError("Hermes installation runs inside the desktop app.");
        setSetupStep("confirm");
        return;
      }

      const installed = await invoke<InstallHermesResult>("install_hermes", {
        distro: setupDistro,
      });
      setSetupDistro(installed.distro);
      appendLog(installed.message);
      appendLog(installed.output || "Hermes install completed");
      setSetupError("Install complete. Click Yes again to create the profile.");
      setSetupStep("confirm");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSetupError(message);
      setSetupAction(classifySetupAction(message));
      setSetupStep("confirm");
      appendLog(`Hermes install failed: ${message}`);
    } finally {
      setVoiceState("idle");
      setMotionActivity("idle");
    }
  }

  function openPanel(panel: Exclude<ActivePanel, null>) {
    setActivePanel((current) => (current === panel ? null : panel));
    if (panel === "workspace" && !runtimeStatus) void checkHermes();
  }

  async function runTerminalCommand(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!identity || terminalBusy) return;
    const command = terminalInput.trim();
    if (!command) return;

    setTerminalBusy(true);
    setTerminalOutput((current) => `${current}${current ? "\n\n" : ""}$ ${command}\n`);

    try {
      const result = await invoke<ProfileCommandResult>("run_profile_command", {
        distro: identity.distro,
        profile: identity.profileSlug,
        command,
      });
      setTerminalOutput((current) => `${current}${result.output || "(no output)"}\n[exit ${result.exitCode}]`);
    } catch (error) {
      setTerminalOutput((current) => `${current}${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setTerminalBusy(false);
    }
  }

  async function runDoctor() {
    if (!identity || doctorBusy) return;
    setDoctorBusy(true);
    setDoctorOutput("");

    try {
      const result = await invoke<HermesCommandResult>("run_hermes_doctor", {
        distro: identity.distro,
        profile: identity.profileSlug,
      });
      setDoctorOutput(result.output || "(no output)");
    } catch (error) {
      setDoctorOutput(error instanceof Error ? error.message : String(error));
    } finally {
      setDoctorBusy(false);
    }
  }

  async function applyHermesProvider() {
    if (!identity || providerBusy) return;
    setProviderBusy(true);
    setProviderOutput("");
    setCodexImportReady(false);

    try {
      const result = await invoke<ConfigureHermesProviderResult>("configure_hermes_provider", {
        distro: identity.distro,
        profile: identity.profileSlug,
        mode: providerChoice,
      });
      setProviderOutput(`${result.message}\n${result.output || "Hermes provider verification completed."}`);
      appendLog(result.message);
      const runtime = await invoke<HermesRuntimeStatus>("check_hermes_runtime", {
        distro: identity.distro,
        profile: identity.profileSlug,
      });
      setRuntimeStatus(runtime);
      setProviderChoice(providerChoiceFromRuntime(runtime));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setProviderOutput(message);
      setCodexImportReady(providerChoice === "codex-subscription" && message.includes("~/.codex/auth.json"));
      appendLog(`Hermes provider setup failed: ${message}`);
    } finally {
      setProviderBusy(false);
    }
  }

  async function importCodexLogin() {
    if (!identity || providerBusy) return;
    setProviderBusy(true);
    setProviderOutput("Importing Codex login into WSL...");

    try {
      const result = await invoke<ImportCodexAuthResult>("import_codex_auth_to_wsl", {
        distro: identity.distro,
        profile: identity.profileSlug,
      });
      appendLog(result.message);
      setProviderOutput(`${result.message}\nApplying Codex provider...`);
      setCodexImportReady(false);
      const provider = await invoke<ConfigureHermesProviderResult>("configure_hermes_provider", {
        distro: identity.distro,
        profile: identity.profileSlug,
        mode: "codex-subscription",
      });
      setProviderOutput(`${provider.message}\n${provider.output || "Hermes provider verification completed."}`);
      appendLog(provider.message);
      const runtime = await invoke<HermesRuntimeStatus>("check_hermes_runtime", {
        distro: identity.distro,
        profile: identity.profileSlug,
      });
      setRuntimeStatus(runtime);
      setProviderChoice(providerChoiceFromRuntime(runtime));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setProviderOutput(message);
      setCodexImportReady(message.includes("one-time") || message.includes("~/.codex/auth.json"));
      appendLog(`Codex login import failed: ${message}`);
    } finally {
      setProviderBusy(false);
    }
  }

  function getAudioContext() {
    if (audioContextRef.current) return audioContextRef.current;
    const AudioContextConstructor =
      window.AudioContext ??
      (window as typeof window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioContextConstructor) return null;
    audioContextRef.current = new AudioContextConstructor();
    return audioContextRef.current;
  }

  function createAnalyser(stream: MediaStream) {
    const context = getAudioContext();
    if (!context) return null;
    void context.resume();
    const source = context.createMediaStreamSource(stream);
    const analyser = context.createAnalyser();
    analyser.fftSize = 1024;
    analyser.smoothingTimeConstant = 0.72;
    source.connect(analyser);
    return { analyser, source };
  }

  function setupLocalMeter(stream: MediaStream) {
    localSourceRef.current?.disconnect();
    const meter = createAnalyser(stream);
    if (!meter) return;
    localAnalyserRef.current = meter.analyser;
    localSourceRef.current = meter.source;
  }

  function setupRemoteMeter(stream: MediaStream) {
    remoteSourceRef.current?.disconnect();
    const meter = createAnalyser(stream);
    if (!meter) return;
    remoteAnalyserRef.current = meter.analyser;
    remoteSourceRef.current = meter.source;
  }

  function readLevel(analyser: AnalyserNode | null, floor: number) {
    if (!analyser) return 0;
    const samples = new Uint8Array(analyser.fftSize);
    analyser.getByteTimeDomainData(samples);
    let sum = 0;
    for (const sample of samples) {
      const centered = (sample - 128) / 128;
      sum += centered * centered;
    }
    const rms = Math.sqrt(sum / samples.length);
    return Math.min(1, Math.max(0, (rms - floor) / 0.18));
  }

  function setMotionActivity(next: VoiceActivity) {
    if (activityRef.current === next) return;
    activityRef.current = next;
    setVoiceActivity(next);
  }

  function startActivityMeter() {
    if (activityFrameRef.current) return;

    const tick = () => {
      const localRaw = readLevel(localAnalyserRef.current, LOCAL_NOISE_FLOOR);
      const remoteRaw = readLevel(remoteAnalyserRef.current, 0.012);
      const nextLocal = levelsRef.current.local * 0.72 + localRaw * 0.28;
      const nextRemote = levelsRef.current.remote * 0.72 + remoteRaw * 0.28;
      levelsRef.current = { local: nextLocal, remote: nextRemote };
      localSpeechFramesRef.current =
        nextLocal > LOCAL_SPEECH_START
          ? Math.min(LOCAL_SPEECH_FRAMES_REQUIRED, localSpeechFramesRef.current + 1)
          : nextLocal < LOCAL_SPEECH_STOP
            ? 0
            : localSpeechFramesRef.current;

      const talkLevel = Math.max(nextLocal, nextRemote);
      if (presenceRef.current) {
        presenceRef.current.style.setProperty("--user-level", nextLocal.toFixed(3));
        presenceRef.current.style.setProperty("--assistant-level", nextRemote.toFixed(3));
        presenceRef.current.style.setProperty("--talk-level", talkLevel.toFixed(3));
        presenceRef.current.style.setProperty("--core-scale", (1 + talkLevel * 0.09).toFixed(3));
        presenceRef.current.style.setProperty("--core-breathe-scale", (1.035 + talkLevel * 0.12).toFixed(3));
        presenceRef.current.style.setProperty("--halo-scale", (1 + nextRemote * 0.13 + nextLocal * 0.07).toFixed(3));
        presenceRef.current.style.setProperty("--halo-breathe-scale", (1.075 + nextRemote * 0.18 + nextLocal * 0.1).toFixed(3));
        presenceRef.current.style.setProperty("--ring-scale", (1 + talkLevel * 0.12).toFixed(3));
        presenceRef.current.style.setProperty("--ring-breathe-scale", (1.045 + talkLevel * 0.16).toFixed(3));
        presenceRef.current.style.setProperty("--voice-glow", (0.08 + nextRemote * 0.22 + nextLocal * 0.14).toFixed(3));
      }

      if (voiceStateRef.current !== "muted" && peerRef.current) {
        if (nextRemote > REMOTE_SPEECH_START && nextRemote > nextLocal * 0.75) {
          setMotionActivity("assistant-speaking");
        } else if (localSpeechFramesRef.current >= LOCAL_SPEECH_FRAMES_REQUIRED) {
          setMotionActivity("user-speaking");
        } else if (activityRef.current !== "connecting") {
          setMotionActivity("listening");
        }
      }

      activityFrameRef.current = window.requestAnimationFrame(tick);
    };

    activityFrameRef.current = window.requestAnimationFrame(tick);
  }

  function stopActivityMeter() {
    if (activityFrameRef.current) {
      window.cancelAnimationFrame(activityFrameRef.current);
      activityFrameRef.current = null;
    }
    localSourceRef.current?.disconnect();
    remoteSourceRef.current?.disconnect();
    localAnalyserRef.current = null;
    remoteAnalyserRef.current = null;
    localSourceRef.current = null;
    remoteSourceRef.current = null;
    levelsRef.current = { local: 0, remote: 0 };
    localSpeechFramesRef.current = 0;
    presenceRef.current?.style.setProperty("--user-level", "0");
    presenceRef.current?.style.setProperty("--assistant-level", "0");
    presenceRef.current?.style.setProperty("--talk-level", "0");
    presenceRef.current?.style.setProperty("--core-scale", "1");
    presenceRef.current?.style.setProperty("--core-breathe-scale", "1.035");
    presenceRef.current?.style.setProperty("--halo-scale", "1");
    presenceRef.current?.style.setProperty("--halo-breathe-scale", "1.075");
    presenceRef.current?.style.setProperty("--ring-scale", "1");
    presenceRef.current?.style.setProperty("--ring-breathe-scale", "1.045");
    presenceRef.current?.style.setProperty("--voice-glow", "0.08");
    void audioContextRef.current?.close();
    audioContextRef.current = null;
  }

  useEffect(() => {
    return () => stopActivityMeter();
  }, []);

  useEffect(() => {
    voiceStateRef.current = voiceState;
  }, [voiceState]);

  if (!bootFinished && !shouldShowOnboarding) {
    return (
      <Suspense
        fallback={
          <main className="os-shell">
            <section className="viewport" />
          </main>
        }
      >
        <BootSequence onFinished={finishBoot} />
      </Suspense>
    );
  }

  return (
    <main className={`os-shell app-shell ${shouldShowOnboarding ? "is-onboarding" : ""} ${surfaceEntering ? "surface-entering" : ""}`}>
      <section className="viewport" aria-label="OS1 voice surface">
        {!shouldShowOnboarding ? (
          <header className="brand-lockup">
            <div className="brand-name">
              <span className="brand-strong">OS</span>
              <span>1</span>
            </div>
            <div className="brand-subtitle">personal operating presence</div>
          </header>
        ) : null}

        {!shouldShowOnboarding ? (
          <div
            ref={presenceRef}
            className={`voice-presence ${surfaceEntering ? "is-entering" : ""} ${isLive ? "is-live" : ""} ${
              isMuted ? "is-muted" : ""
            } is-${voiceActivity}`}
            aria-label={`Voice presence is ${voiceActivity.replace("-", " ")}`}
          >
            <div className="voice-halo" />
            <div className="voice-aura aura-one" />
            <div className="voice-aura aura-two" />
            <div className="voice-core">
              <AudioLines size={46} strokeWidth={1.1} />
            </div>
            <div className="voice-ring ring-one" />
            <div className="voice-ring ring-two" />
          </div>
        ) : null}

        {!shouldShowOnboarding ? (
          <aside className="status-panel" aria-label="System status">
            <div>
              <span className="status-kicker">{identity ? identity.assistantName : "Hermes"}</span>
              {topStatusMessage ? <p>{topStatusMessage}</p> : null}
            </div>
            <button className="text-action" type="button" onClick={checkHermes}>
              check
            </button>
          </aside>
        ) : null}

        {shouldShowOnboarding ? (
          <section className="naming-panel" aria-label="First run setup">
            {setupStep === "name" ? (
              <form onSubmit={beginNameConfirmation}>
                <span className="setup-kicker">First presence</span>
                <h1>What would you like to call me?</h1>
                <div className="name-entry">
                  <input
                    autoFocus
                    value={nameInput}
                    onChange={(event) => setNameInput(event.target.value)}
                    placeholder="Samantha"
                    aria-label="Assistant name"
                  />
                  <button type="submit">Submit</button>
                </div>
              </form>
            ) : setupStep === "creating" || setupStep === "discovering" ? (
              <div className="setup-loading" aria-live="polite" aria-busy="true">
                <span className="setup-kicker">{setupStep === "discovering" ? "Finding presence" : "Creating presence"}</span>
                <div className="setup-orbit" aria-hidden="true">
                  <span />
                  <span />
                  <span />
                </div>
                <h1>{pendingName || "OS1"}</h1>
                <p>{setupStep === "discovering" ? "Looking for existing Hermes profiles" : "Preparing Hermes profile"}</p>
              </div>
            ) : (
              <div>
                <span className="setup-kicker">Confirm</span>
                <h1>Call me {pendingName}?</h1>
                <p>Hermes profile: {slugifyProfileName(pendingName)}</p>
                <div className="provider-options" role="radiogroup" aria-label="Hermes model source">
                  <button
                    className={providerChoice === "codex-subscription" ? "provider-option selected" : "provider-option"}
                    type="button"
                    role="radio"
                    aria-checked={providerChoice === "codex-subscription"}
                    onClick={() => setProviderChoice("codex-subscription")}
                  >
                    <span>Codex subscription</span>
                    <small>ChatGPT OAuth</small>
                  </button>
                  <button
                    className={providerChoice === "openai-key" ? "provider-option selected" : "provider-option"}
                    type="button"
                    role="radio"
                    aria-checked={providerChoice === "openai-key"}
                    onClick={() => setProviderChoice("openai-key")}
                  >
                    <span>OpenAI key</span>
                    <small>GPT-5.5</small>
                  </button>
                </div>
                <div className="confirm-actions">
                  <button className="secondary-choice" type="button" onClick={() => setSetupStep("name")}>
                    Change
                  </button>
                  <button type="button" onClick={confirmAssistantName}>
                    Yes
                  </button>
                </div>
              </div>
            )}
            {setupError ? (
              <div className="setup-error-block">
                <p className="setup-error">{setupError}</p>
                {setupAction === "install" ? (
                  <button className="repair-control" type="button" onClick={installHermes} disabled={setupStep === "creating"}>
                    Install Hermes
                  </button>
                ) : setupAction === "repair" ? (
                  <button className="repair-control" type="button" onClick={repairHermes} disabled={setupStep === "creating"}>
                    Repair Hermes
                  </button>
                ) : null}
              </div>
            ) : null}
          </section>
        ) : null}

        {!shouldShowOnboarding ? (
        <footer className="control-strip" aria-label="Voice controls">
          {!isLive && !isMuted ? (
            <button className={`primary-control ${isStartingVoice ? "is-loading" : ""}`} type="button" onClick={startVoice} disabled={isStartingVoice}>
              {isStartingVoice ? <span className="control-spinner" aria-hidden="true" /> : <Mic size={18} />}
              <span>{isStartingVoice ? "Connecting" : "Begin"}</span>
            </button>
          ) : (
            <>
              <button className="icon-control" type="button" aria-label={isMuted ? "Unmute" : "Mute"} onClick={toggleMute}>
                {isMuted ? <MicOff size={18} /> : <Mic size={18} />}
              </button>
              <button className="icon-control" type="button" aria-label="Stop voice" onClick={() => stopVoice()}>
                <Pause size={18} />
              </button>
            </>
          )}
          <button className="icon-control secondary" type="button" aria-label="Open terminal" onClick={() => void openPanel("terminal")}>
            <Terminal size={17} />
          </button>
          <button className="icon-control secondary" type="button" aria-label="Open workspace" onClick={() => void openPanel("workspace")}>
            <Settings size={17} />
          </button>
        </footer>
        ) : null}

        {!shouldShowOnboarding && activePanel ? (
          <aside className="workspace-drawer" aria-label={activePanel === "terminal" ? "Terminal" : "Workspace"}>
            <div className="drawer-header">
              <div>
                <span className="status-kicker">{activePanel === "terminal" ? "Terminal" : "Workspace"}</span>
                <h2>{identity?.assistantName}</h2>
              </div>
              <button className="text-action" type="button" onClick={() => setActivePanel(null)}>
                close
              </button>
            </div>

            {activePanel === "workspace" ? (
              <div className="workspace-content">
                <div className="runtime-grid">
                  {runtimeRows.map((row) => (
                    <div className="runtime-row" key={row.label}>
                      <span>{row.label}</span>
                      <strong>{row.value}</strong>
                    </div>
                  ))}
                </div>
                <section className="provider-section" aria-label="Hermes provider">
                  <div>
                    <span className="status-kicker">Provider</span>
                    <p>{providerChoice === "openai-key" ? "OpenAI key · GPT-5.5" : "Codex subscription · ChatGPT OAuth"}</p>
                  </div>
                  <div className="drawer-provider-options" role="radiogroup" aria-label="Hermes model source">
                    <button
                      className={providerChoice === "codex-subscription" ? "provider-option selected" : "provider-option"}
                      type="button"
                      role="radio"
                      aria-checked={providerChoice === "codex-subscription"}
                      onClick={() => setProviderChoice("codex-subscription")}
                      disabled={providerBusy}
                    >
                      <span>Codex subscription</span>
                      <small>ChatGPT OAuth</small>
                    </button>
                    <button
                      className={providerChoice === "openai-key" ? "provider-option selected" : "provider-option"}
                      type="button"
                      role="radio"
                      aria-checked={providerChoice === "openai-key"}
                      onClick={() => setProviderChoice("openai-key")}
                      disabled={providerBusy}
                    >
                      <span>OpenAI key</span>
                      <small>GPT-5.5</small>
                    </button>
                  </div>
                  <button className="panel-action" type="button" onClick={applyHermesProvider} disabled={providerBusy}>
                    {providerBusy ? "Applying" : "Apply"}
                  </button>
                  {codexImportReady ? (
                    <button className="panel-action secondary-panel-action" type="button" onClick={importCodexLogin} disabled={providerBusy}>
                      Import Codex Login
                    </button>
                  ) : null}
                  {providerOutput ? <pre className="panel-output provider-output">{providerOutput}</pre> : null}
                </section>
                <section className="provider-section" aria-label="Voice personality">
                  <div>
                    <span className="status-kicker">Personality</span>
                    <p>{personalityMode === "her" ? "Her - intimate companion mode" : "Assistant - warm practical mode"}</p>
                  </div>
                  <div className="drawer-provider-options" role="radiogroup" aria-label="Voice personality mode">
                    <button
                      className={personalityMode === "assistant" ? "provider-option selected" : "provider-option"}
                      type="button"
                      role="radio"
                      aria-checked={personalityMode === "assistant"}
                      onClick={() => updatePersonalityMode("assistant")}
                    >
                      <span>Assistant</span>
                      <small>Practical</small>
                    </button>
                    <button
                      className={personalityMode === "her" ? "provider-option selected" : "provider-option"}
                      type="button"
                      role="radio"
                      aria-checked={personalityMode === "her"}
                      onClick={() => updatePersonalityMode("her")}
                    >
                      <span>Her</span>
                      <small>Companion</small>
                    </button>
                  </div>
                </section>
                <button className="panel-action" type="button" onClick={runDoctor} disabled={doctorBusy}>
                  {doctorBusy ? "Running" : "Doctor"}
                </button>
                {doctorOutput ? <pre className="panel-output">{doctorOutput}</pre> : null}
              </div>
            ) : (
              <div className="terminal-content">
                <pre className="panel-output terminal-output">{terminalOutput || profilePrompt(identity)}</pre>
                <form className="terminal-entry" onSubmit={runTerminalCommand}>
                  <input
                    value={terminalInput}
                    onChange={(event) => setTerminalInput(event.target.value)}
                    disabled={terminalBusy}
                    aria-label="Profile command"
                  />
                  <button type="submit" disabled={terminalBusy}>
                    {terminalBusy ? "Run" : "Send"}
                  </button>
                </form>
              </div>
            )}
          </aside>
        ) : null}

      </section>
    </main>
  );
}

function loadAssistantIdentity() {
  try {
    const raw = window.localStorage.getItem(IDENTITY_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as AssistantIdentity;
    if (!parsed.assistantName || !parsed.profileSlug || !parsed.distro) return null;
    return parsed;
  } catch {
    return null;
  }
}

function saveAssistantIdentity(identity: AssistantIdentity) {
  window.localStorage.setItem(IDENTITY_STORAGE_KEY, JSON.stringify(identity));
}

function loadPersonalityMode(): PersonalityMode {
  const value = window.localStorage.getItem(PERSONALITY_STORAGE_KEY);
  return value === "her" ? "her" : "assistant";
}

function savePersonalityMode(mode: PersonalityMode) {
  window.localStorage.setItem(PERSONALITY_STORAGE_KEY, mode);
}

function slugifyProfileName(name: string) {
  return name
    .trim()
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48);
}

function profileNameForDisplay(profileName: string) {
  const words = profileName
    .split(/[-_\s]+/)
    .map((word) => word.trim())
    .filter(Boolean);
  if (!words.length) return "OS1";
  return words.map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join(" ");
}

function buildRealtimeTools(identity: AssistantIdentity | null) {
  if (!identity) return [];
  return [
    {
      type: "function",
      name: "check_hermes_status",
      description: "Check whether the selected Hermes profile runtime is reachable and ready.",
      parameters: {
        type: "object",
        properties: {},
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "ask_hermes",
      description: "Ask the selected Hermes profile a concise question through Hermes chat.",
      parameters: {
        type: "object",
        properties: {
          prompt: {
            type: "string",
            description: "A concise prompt to send to Hermes.",
          },
        },
        required: ["prompt"],
        additionalProperties: false,
      },
    },
  ];
}

function parseFunctionArguments(raw: string | undefined) {
  if (!raw) return {} as Record<string, unknown>;
  try {
    const parsed = JSON.parse(raw) as unknown;
    return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

function makeRuntimeRows(status: HermesRuntimeStatus) {
  return [
    { label: "Profile", value: status.profile },
    { label: "Distro", value: status.distro },
    { label: "Home", value: status.hermesHome },
    { label: "Hermes", value: status.version ?? status.hermesCommand ?? "missing" },
    { label: "Model", value: status.modelDefault ?? "unknown" },
    { label: "Provider", value: status.modelProvider ?? "unknown" },
    { label: "Config", value: status.hasConfig ? "ready" : "missing" },
    { label: "Env", value: status.hasEnv ? "ready" : "missing" },
    { label: "Sessions", value: status.hasSessions ? "ready" : "missing" },
    { label: "Skills", value: status.hasSkills ? "ready" : "missing" },
    { label: "Cron", value: status.hasCron ? "ready" : "missing" },
  ];
}

function providerChoiceFromRuntime(status: HermesRuntimeStatus): ProviderChoice {
  return status.modelProvider === "openai-codex" ? "codex-subscription" : "openai-key";
}

function profilePrompt(identity: AssistantIdentity | null) {
  if (!identity) return "";
  return `$ HERMES_HOME=~/.hermes/profiles/${identity.profileSlug}\n`;
}

function classifySetupAction(message: string): SetupAction {
  const lowered = message.toLowerCase();
  if (
    lowered.includes("cli was not found") ||
    lowered.includes("hermes checkout was not found") ||
    lowered.includes("no local hermes") ||
    lowered.includes("install hermes") ||
    lowered.includes("hermes cli")
  ) {
    return "install";
  }
  if (
    lowered.includes("module") ||
    lowered.includes("uv was not found") ||
    lowered.includes("repair") ||
    lowered.includes("python") ||
    lowered.includes("prompt_toolkit") ||
    lowered.includes("yaml")
  ) {
    return "repair";
  }
  return null;
}
