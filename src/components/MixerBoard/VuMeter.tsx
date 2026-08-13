import { useEffect, useRef } from "react";
import { perceptual } from "../../lib/audio";
import { sleepMeter, subscribeLevel, wakeMeter } from "../../lib/liveMeters";
import { meterFrameInterval, meterNeedsFrame } from "../../lib/meter";
import { useMixerStore } from "../../store/mixer";

interface VuMeterProps {
  /** LevelStore name emitted by the native backend. */
  source: string;
  /** False settles the visual to zero without affecting the audio engine. */
  enabled: boolean;
  /** Microphones use their mono/left peak; playback meters use louder L/R. */
  mono?: boolean;
}

/** Meter height (0-1) for a dBFS value, matching the sqrt display curve:
 * height = sqrt(amplitude) = 10^(dB/40). */
const heightForDb = (db: number) => Math.pow(10, db / 40);

/** −6 dBFS reference tick (the red zone above −3 dBFS lives in the CSS
 * gradient stops, which use the same mapping). */
const TICK_6DB = heightForDb(-6) * 100; // ≈ 70.8%
/** Clip latch threshold ≈ −0.2 dBFS (in height space). */
const CLIP_AT = heightForDb(-0.2);

/**
 * Live level meter, calibrated in dBFS. Targets arrive at 10 Hz from the
 * backend's `levels` events; an adaptive animation smooths toward them (fast
 * attack, slow release) outside React state. Its rate follows the user's
 * visual-quality preference and it stops scheduling work once fully silent.
 * Green below −6 dB, amber to −3 dB, red above - and a clip light that
 * latches for 1.5 s when the signal touches 0 dBFS. The readout shows the held
 * peak in dBFS.
 * Under the pactl fallback no events arrive and the meter rests at zero.
 */
export function VuMeter({ source, enabled, mono = false }: Readonly<VuMeterProps>) {
  const mode = useMixerStore((state) => state.meterMode);
  const fillRef = useRef<HTMLDivElement>(null);
  const peakRef = useRef<HTMLDivElement>(null);
  const clipRef = useRef<HTMLDivElement>(null);
  const dbRef = useRef<HTMLDivElement>(null);
  const motionRef = useRef({
    smooth: 0,
    peak: 0,
    clipUntil: 0,
    lastFill: "",
    lastPeak: "",
    lastClip: "",
    lastDbText: "",
  });

  useEffect(() => {
    let target = 0;
    let lastFrame = performance.now();
    const interval = meterFrameInterval(mode);

    const paint = (smooth: number, peak: number, clipOn: boolean) => {
      const motion = motionRef.current;
      const fill = `inset(${(100 - smooth * 100).toFixed(1)}% 0 0 0)`;
      const peakBottom = `${(peak * 100).toFixed(1)}%`;
      const clipClass = "vu-clip" + (clipOn ? " on" : "");
      if (fillRef.current && fill !== motion.lastFill) {
        motion.lastFill = fill;
        fillRef.current.style.clipPath = fill;
      }
      if (peakRef.current && peakBottom !== motion.lastPeak) {
        motion.lastPeak = peakBottom;
        peakRef.current.style.bottom = peakBottom;
      }
      if (clipRef.current && clipClass !== motion.lastClip) {
        motion.lastClip = clipClass;
        clipRef.current.className = clipClass;
      }
      if (dbRef.current) {
        const text = peak < 0.02 ? "−∞" : String(Math.round(40 * Math.log10(peak)));
        if (text !== motion.lastDbText) {
          motion.lastDbText = text;
          dbRef.current.textContent = text;
        }
      }
    };

    if (mode === "off") {
      paint(0, 0, false);
      motionRef.current.smooth = 0;
      motionRef.current.peak = 0;
      motionRef.current.clipUntil = 0;
      return;
    }

    const tick = (now: number) => {
      const elapsed = now - lastFrame;
      lastFrame = now;
      const frameScale = Math.max(0.25, Math.min(6, elapsed / (1000 / 60)));
      const motion = motionRef.current;
      const smoothing = target > motion.smooth ? 0.5 : 0.12;
      motion.smooth += (target - motion.smooth) * (1 - Math.pow(1 - smoothing, frameScale));
      motion.peak = Math.max(motion.peak * Math.pow(0.985, frameScale), motion.smooth);
      if (target >= CLIP_AT) motion.clipUntil = now + 1500;
      const clipOn = now < motion.clipUntil;

      paint(motion.smooth, motion.peak, clipOn);
      const moving = meterNeedsFrame(mode, target, motion.smooth, motion.peak, clipOn);
      if (!moving && target <= 0.001) {
        motion.smooth = 0;
        motion.peak = 0;
        paint(0, 0, false);
      }
      return moving;
    };

    const unsubscribe = subscribeLevel(source, (level) => {
      if (!enabled) return;
      const amplitude = mono ? level[0] : Math.max(level[0], level[1]);
      const nextTarget = perceptual(amplitude);
      if (Math.abs(nextTarget - target) < 0.0001) return;
      target = nextTarget;
      wakeMeter(tick, interval);
    });
    // Apply mute/disable changes immediately instead of waiting for the next
    // native peak event.
    const motion = motionRef.current;
    if (!enabled && (motion.smooth > 0 || motion.peak > 0 || motion.clipUntil > performance.now())) {
      wakeMeter(tick, interval);
    }
    return () => {
      unsubscribe();
      sleepMeter(tick);
    };
  }, [enabled, mode, mono, source]);

  return (
    <div className={`vu-col${mode === "off" ? " disabled" : ""}`} title={mode === "off" ? "Live meters are disabled in Settings" : "Peak level in dBFS - tick at −6, red above −3, light latches on clipping"}>
      <div className="vu-clip" ref={clipRef} />
      <div className="meter">
        <div className="meter-fill" ref={fillRef} />
        <div className="meter-tick" style={{ bottom: `${TICK_6DB}%` }} />
        <div className="meter-peak" ref={peakRef} style={{ bottom: "0%" }} />
      </div>
      <div className="vu-db" ref={dbRef}>
        −∞
      </div>
    </div>
  );
}
