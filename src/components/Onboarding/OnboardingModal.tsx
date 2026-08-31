import { useEffect, useRef, useState, type ReactNode } from "react";
import { useMixerStore } from "../../store/mixer";
import { useI18n, type TranslationKey } from "../../i18n";
import { Ms } from "../Icons";
import { Modal } from "../Modal";

type PreviewKind = "mixer" | "apps" | "profiles" | "microphone";

interface Step {
  icon: string;
  title: TranslationKey;
  body: TranslationKey;
  preview: PreviewKind;
}

function PreviewFrame({
  icon,
  title,
  children,
}: Readonly<{ icon: string; title: string; children: ReactNode }>) {
  return (
    <div className="ob-preview-frame" aria-hidden="true">
      <div className="ob-preview-bar">
        <span className="ob-preview-brand"><span /></span>
        <Ms name={icon} />
        <span>{title}</span>
        <span className="ob-preview-status"><i /> Sonux</span>
      </div>
      {children}
    </div>
  );
}

function MixerPreview() {
  const { t } = useI18n();
  const strips = [
    { icon: "sports_esports", label: t("onboarding.preview.game"), level: 72 },
    { icon: "forum", label: t("onboarding.preview.chat"), level: 48 },
    { icon: "music_note", label: t("onboarding.preview.media"), level: 61 },
  ];
  return (
    <PreviewFrame icon="graphic_eq" title={t("navigation.mixer")}>
      <div className="ob-mini-mixer">
        <div className="ob-mini-section">
          <span>{t("mixer.group.channels")}</span><Ms name="add" />
        </div>
        <div className="ob-mini-strips">
          {strips.map((strip) => (
            <div className="ob-mini-strip" key={strip.label}>
              <Ms name={strip.icon} />
              <strong>{strip.label}</strong>
              <div className="ob-mini-fader"><i style={{ height: `${strip.level}%` }} /></div>
              <small>{strip.level}%</small>
            </div>
          ))}
          <div className="ob-mini-strip is-mix">
            <Ms name="radio_button_checked" />
            <strong>{t("onboarding.preview.streamMix")}</strong>
            <div className="ob-mini-fader"><i style={{ height: "67%" }} /></div>
            <small>67%</small>
          </div>
        </div>
      </div>
    </PreviewFrame>
  );
}

function AppsPreview() {
  const { t } = useI18n();
  return (
    <PreviewFrame icon="apps" title={t("navigation.applications")}>
      <div className="ob-mini-apps">
        <div className="ob-mini-section"><span>{t("onboarding.preview.running")}</span><small>2</small></div>
        <div className="ob-mini-app-row">
          <span className="ob-mini-app-icon"><Ms name="sports_esports" /></span>
          <span><strong>{t("onboarding.preview.game")}</strong><small>{t("onboarding.preview.playing")}</small></span>
          <Ms name="arrow_forward" className="ob-direction-arrow" />
          <b><Ms name="sports_esports" /><span>{t("onboarding.preview.game")}</span></b>
        </div>
        <div className="ob-mini-app-row">
          <span className="ob-mini-app-icon"><Ms name="language" /></span>
          <span><strong>{t("onboarding.preview.browser")}</strong><small>{t("onboarding.preview.playing")}</small></span>
          <Ms name="arrow_forward" className="ob-direction-arrow" />
          <b><Ms name="music_note" /><span>{t("onboarding.preview.media")}</span></b>
        </div>
        <div className="ob-mini-memory"><Ms name="check_circle" /> {t("onboarding.preview.routeRemembered")}</div>
      </div>
    </PreviewFrame>
  );
}

function ProfilesPreview() {
  const { t } = useI18n();
  return (
    <PreviewFrame icon="bookmarks" title={t("navigation.profiles")}>
      <div className="ob-mini-profiles">
        <div className="ob-mini-profile-list">
          <span className="is-active"><i />{t("onboarding.preview.gaming")}</span>
          <span>{t("onboarding.preview.everyday")}</span>
          <span>{t("onboarding.preview.streaming")}</span>
        </div>
        <div className="ob-mini-profile-detail">
          <div><strong>{t("onboarding.preview.gaming")}</strong><span><i /> {t("profiles.active")}</span></div>
          <p>{t("profiles.applications.description", { profile: t("onboarding.preview.gaming") })}</p>
          <div className="ob-mini-link"><Ms name="sports_esports" /><strong>{t("onboarding.preview.game")}</strong><small>{t("onboarding.preview.autoSwitch")}</small></div>
          <div className="ob-mini-saved"><Ms name="check" /> {t("onboarding.preview.profileSaved")}</div>
        </div>
      </div>
    </PreviewFrame>
  );
}

function MicrophonePreview() {
  const { t } = useI18n();
  const processors = [
    { icon: "noise_control_off", label: t("processing.noiseGate"), width: 58 },
    { icon: "compress", label: t("processing.compressor"), width: 72 },
    { icon: "vertical_align_top", label: t("processing.limiter"), width: 86 },
  ];
  return (
    <PreviewFrame icon="mic" title={t("microphone.title")}>
      <div className="ob-mini-mic">
        <div className="ob-mini-mic-head"><span><Ms name="mic" /> {t("microphone.processed")}</span><i /></div>
        {processors.map((processor) => (
          <div className="ob-mini-processor" key={processor.label}>
            <Ms name={processor.icon} />
            <strong>{processor.label}</strong>
            <div><i style={{ width: `${processor.width}%` }} /></div>
            <span className="ob-mini-toggle" />
          </div>
        ))}
        <div className="ob-mini-memory"><Ms name="headphones" /> {t("onboarding.preview.readyInApps")}</div>
      </div>
    </PreviewFrame>
  );
}

function StepPreview({ kind }: Readonly<{ kind: PreviewKind }>) {
  switch (kind) {
    case "mixer": return <MixerPreview />;
    case "apps": return <AppsPreview />;
    case "profiles": return <ProfilesPreview />;
    case "microphone": return <MicrophonePreview />;
  }
}

const STEPS: Step[] = [
  { icon: "graphic_eq", title: "onboarding.setup.title", body: "onboarding.setup.body", preview: "mixer" },
  { icon: "grid_view", title: "onboarding.apps.title", body: "onboarding.apps.body", preview: "apps" },
  { icon: "bookmarks", title: "onboarding.profiles.title", body: "onboarding.profiles.body", preview: "profiles" },
  { icon: "mic", title: "onboarding.microphone.title", body: "onboarding.microphone.body", preview: "microphone" },
];

function ObProgress({ step, onSelect }: Readonly<{ step: number; onSelect: (step: number) => void }>) {
  const { t } = useI18n();
  const total = STEPS.length + 1;
  return (
    <div className="ob-progress">
      <span>{t("onboarding.progress", { current: step + 1, total })}</span>
      <div className="ob-dots">
        {[...STEPS, null].map((item, index) => (
          <button
            type="button"
            key={item?.title ?? "choice"}
            className={index === step ? "on" : ""}
            aria-label={t("onboarding.progressGoTo", { current: index + 1, total })}
            aria-current={index === step ? "step" : undefined}
            onClick={() => onSelect(index)}
          />
        ))}
      </div>
    </div>
  );
}

/** First-run orientation: four concise product previews, then a starting setup choice. */
export function OnboardingModal() {
  const { t } = useI18n();
  const show = useMixerStore((state) => state.showOnboarding);
  const replay = useMixerStore((state) => state.onboardingReplay);
  const finishOnboarding = useMixerStore((state) => state.finishOnboarding);
  const [step, setStep] = useState(0);
  const stepHeading = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    if (show) setStep(0);
  }, [show]);

  useEffect(() => {
    if (show) stepHeading.current?.focus();
  }, [show, step]);

  const last = step === STEPS.length;
  const current = STEPS[step];

  return (
    <Modal
      open={show}
      onClose={() => void finishOnboarding(false)}
      title={t("onboarding.dialogLabel")}
      className="ob-modal"
      dismissible={replay}
    >
      {last ? (
        <div className="ob-final">
          <div className="ob-final-mark"><Ms name={replay ? "check" : "dashboard_customize"} /></div>
          <div className="ob-copy">
            <h2 ref={stepHeading} tabIndex={-1}>{t(replay ? "onboarding.replay.title" : "onboarding.choice.title")}</h2>
            <p>{t(replay ? "onboarding.replay.body" : "onboarding.choice.body")}</p>
          </div>
          {!replay && (
            <div className="ob-choices">
              <button type="button" className="ob-choice" onClick={() => void finishOnboarding(false)}>
                <Ms name="dashboard" />
                <span><strong>{t("onboarding.choice.ready.title")}</strong><small>{t("onboarding.choice.ready.body")}</small></span>
                <Ms name="arrow_forward" className="ob-direction-arrow" />
              </button>
              <button type="button" className="ob-choice" onClick={() => void finishOnboarding(true)}>
                <Ms name="add_box" />
                <span><strong>{t("onboarding.choice.custom.title")}</strong><small>{t("onboarding.choice.custom.body")}</small></span>
                <Ms name="arrow_forward" className="ob-direction-arrow" />
              </button>
            </div>
          )}
        </div>
      ) : (
        <>
          <div className="ob-copy">
            <span className="ob-step-icon"><Ms name={current.icon} /></span>
            <div>
              <h2 ref={stepHeading} tabIndex={-1}>{t(current.title)}</h2>
              <p>{t(current.body)}</p>
            </div>
          </div>
          <div className="ob-preview" key={current.preview}>
            <StepPreview kind={current.preview} />
          </div>
        </>
      )}
      <div className="ob-foot">
        <button
          type="button"
          className="modal-btn"
          onClick={() => step > 0 ? setStep(step - 1) : void finishOnboarding(false)}
        >
          {t(step > 0 ? "common.action.back" : "common.action.skip")}
        </button>
        <ObProgress step={step} onSelect={setStep} />
        {last && replay ? (
          <button type="button" className="modal-btn primary" onClick={() => void finishOnboarding(false)}>
            {t("common.action.done")}
          </button>
        ) : last ? <span className="ob-foot-spacer" /> : (
          <button type="button" className="modal-btn primary" onClick={() => setStep(step + 1)}>
            {t("common.action.next")}
          </button>
        )}
      </div>
    </Modal>
  );
}
