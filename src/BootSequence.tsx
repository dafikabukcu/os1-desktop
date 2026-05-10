import { useEffect, useRef, useState } from "react";
import { InfinityLoader } from "./infinity-loader.js";

type BootSequenceProps = {
  onFinished: () => void;
};

const LOAD_LEAD_IN_MS = 700;
const TAGLINE_DELAY_MS = 450;
const HELIX_START_MS = 100;
const TRIGGER_TRANSITION_MS = 1250;
const MORPH_TO_DOT_MS = 780;
const HELIX_TO_RING_MS = 13_050;
const AUDIO_FALLBACK_MS = 14_250;
const TAGLINE = "We believe in infinity";

function delay(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function playInitSound() {
  const audio = new Audio("/startup.mp3");
  audio.preload = "auto";

  let settled = false;
  let fallbackId = 0;

  const ended = new Promise<void>((resolve) => {
    const finish = () => {
      if (settled) return;
      settled = true;
      window.clearTimeout(fallbackId);
      resolve();
    };

    audio.addEventListener("ended", finish, { once: true });
    audio.addEventListener("error", finish, { once: true });
    fallbackId = window.setTimeout(finish, AUDIO_FALLBACK_MS);
  });

  void audio.play().catch(() => undefined);
  return { audio, ended };
}

export function BootSequence({ onFinished }: BootSequenceProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const loaderRef = useRef<InfinityLoader | null>(null);
  const bootAudioRef = useRef<HTMLAudioElement | null>(null);
  const finishedRef = useRef(false);
  const [brandVisible, setBrandVisible] = useState(false);
  const [taglineVisible, setTaglineVisible] = useState(false);
  const [fadingChrome, setFadingChrome] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const canvas = canvasRef.current;
    if (!canvas) return undefined;

    const loader = new InfinityLoader(canvas, {
      transitionDurationMs: HELIX_TO_RING_MS,
      maxCanvasSize: 560,
    });
    loaderRef.current = loader;

    async function runBoot() {
      setBrandVisible(true);
      await delay(LOAD_LEAD_IN_MS);
      if (cancelled) return;

      window.setTimeout(() => setTaglineVisible(true), TAGLINE_DELAY_MS);
      window.setTimeout(() => loader.start(), HELIX_START_MS);

      const helixComplete = new Promise<void>((resolve) => {
        loader.onComplete = () => resolve();
      });

      await delay(TRIGGER_TRANSITION_MS);
      if (cancelled) return;

      const { audio, ended: audioEnded } = playInitSound();
      bootAudioRef.current = audio;
      loader.triggerTransition();
      await helixComplete;
      if (cancelled) return;

      setFadingChrome(true);
      await loader.morphToDot(MORPH_TO_DOT_MS);
      await audioEnded;
      finish();
    }

    function finish() {
      if (finishedRef.current) return;
      finishedRef.current = true;
      bootAudioRef.current?.pause();
      bootAudioRef.current = null;
      onFinished();
    }

    void runBoot();

    return () => {
      cancelled = true;
      bootAudioRef.current?.pause();
      bootAudioRef.current = null;
      loader.destroy();
    };
  }, [onFinished]);

  function skipBoot() {
    if (finishedRef.current) return;
    finishedRef.current = true;
    bootAudioRef.current?.pause();
    bootAudioRef.current = null;
    loaderRef.current?.destroy();
    onFinished();
  }

  return (
    <main className="os-shell boot-shell" onClick={skipBoot}>
      <section className="viewport boot-viewport" aria-label="OS1 boot sequence">
        <canvas ref={canvasRef} className="boot-canvas" />
        <div className={`boot-lockup ${brandVisible ? "visible" : ""} ${fadingChrome ? "fading" : ""}`}>
          <div className="boot-brand-name">
            <span className="boot-brand-strong">OS</span>
            <span>1</span>
          </div>
          <div className="boot-brand-subtitle">computer use</div>
        </div>
        <p className={`boot-tagline ${fadingChrome ? "fading" : ""}`} aria-label={TAGLINE}>
          {TAGLINE.split("").map((character, index) => (
            <span
              className={taglineVisible ? "revealed" : ""}
              key={`${character}-${index}`}
              style={{ transitionDelay: `${index * 35}ms` }}
            >
              {character === " " ? "\u00a0" : character}
            </span>
          ))}
        </p>
      </section>
    </main>
  );
}
