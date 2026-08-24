import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  AlertTriangle,
  CircleCheck,
  Clapperboard,
  Download,
  FolderOpen,
  Loader2,
  Wand2,
} from "lucide-react";

import { availableLangs, getLang, setLang, t } from "./i18n";

type AppStatus = {
  ffmpeg_available: boolean;
  ffmpeg_path: string | null;
  model_dir: string;
  installed_model: string | null;
  models: { id: string; installed: boolean; approx_bytes: number }[];
};

type StyleInfo = {
  id: string;
  base_color: string;
  highlight_color: string;
  uppercase: boolean;
  alignment: number;
  max_words_per_line: number;
};

type Progress = { stage: string; percent: number };
type CommandError = { code: string; detail: string };

type Phase = "idle" | "working" | "done" | "error";

const SPOKEN_LANGS = ["auto", "de", "en"] as const;

function basename(p: string): string {
  const parts = p.split("/");
  return parts[parts.length - 1] || p;
}

function asCommandError(e: unknown): CommandError {
  if (e && typeof e === "object" && "code" in e) return e as CommandError;
  return { code: "unknown", detail: String(e) };
}

export default function App() {
  const [uiLang, setUiLang] = useState(getLang());
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [styles, setStyles] = useState<StyleInfo[]>([]);
  const [styleId, setStyleId] = useState("bold-center");
  const [spokenLang, setSpokenLang] = useState<string>("auto");

  const [video, setVideo] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);

  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<Progress>({ stage: "probing", percent: 0 });
  const [output, setOutput] = useState<string | null>(null);
  const [error, setError] = useState<CommandError | null>(null);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [downloadPct, setDownloadPct] = useState(0);

  // Rerender the whole tree when the UI language changes; the locale layer is
  // module state, not React state, so a version counter is the trigger.
  const rerenderKey = uiLang;

  const refreshStatus = useCallback(() => {
    invoke<AppStatus>("app_status").then(setStatus).catch(() => setStatus(null));
  }, []);

  useEffect(() => {
    refreshStatus();
    invoke<StyleInfo[]>("list_styles")
      .then((list) => {
        setStyles(list);
        if (list.length > 0) setStyleId((cur) => (list.some((s) => s.id === cur) ? cur : list[0].id));
      })
      .catch(() => setStyles([]));
  }, [refreshStatus]);

  useEffect(() => {
    const unlisten = listen<Progress>("wortlaut://progress", (e) => setProgress(e.payload));
    return () => {
      unlisten.then((f) => f()).catch(() => undefined);
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<{ percent: number }>("wortlaut://download", (e) =>
      setDownloadPct(e.payload.percent),
    );
    return () => {
      unlisten.then((f) => f()).catch(() => undefined);
    };
  }, []);

  // Native drag and drop. Guarded because the same bundle also runs in a plain
  // browser during UI work, where there is no Tauri webview to talk to.
  useEffect(() => {
    let stop: (() => void) | undefined;
    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          const p = event.payload;
          if (p.type === "over") {
            setDragging(true);
          } else if (p.type === "drop") {
            setDragging(false);
            const first = p.paths[0];
            if (first) {
              setVideo(first);
              setPhase("idle");
              setOutput(null);
              setError(null);
            }
          } else {
            setDragging(false);
          }
        })
        .then((f) => {
          stop = f;
        })
        .catch(() => undefined);
    } catch {
      // Not running inside Tauri.
    }
    return () => stop?.();
  }, []);

  const chooseVideo = useCallback(async () => {
    const picked = await invoke<string | null>("pick_video").catch(() => null);
    if (picked) {
      setVideo(picked);
      setPhase("idle");
      setOutput(null);
      setError(null);
    }
  }, []);

  const start = useCallback(async () => {
    if (!video) return;
    setPhase("working");
    setError(null);
    setOutput(null);
    setProgress({ stage: "probing", percent: 0 });
    try {
      const out = await invoke<string>("process_video", {
        path: video,
        styleId,
        language: spokenLang,
      });
      setOutput(out);
      setPhase("done");
    } catch (e) {
      setError(asCommandError(e));
      setPhase("error");
    }
  }, [video, styleId, spokenLang]);

  const download = useCallback(
    async (id: string) => {
      setDownloading(id);
      setDownloadPct(0);
      try {
        await invoke<string>("download_model", { id });
        refreshStatus();
      } catch (e) {
        setError(asCommandError(e));
        setPhase("error");
      } finally {
        setDownloading(null);
      }
    },
    [refreshStatus],
  );

  const switchLang = useCallback((code: string) => {
    setLang(code);
    localStorage.setItem("wortlaut.lang", code);
    setUiLang(code);
  }, []);

  const needsFfmpeg = status !== null && !status.ffmpeg_available;
  const needsModel = status !== null && status.installed_model === null;
  const canStart = Boolean(video) && !needsFfmpeg && !needsModel && phase !== "working";

  return (
    <div className="shell" key={rerenderKey}>
      <header className="masthead">
        <div>
          <h1>{t("app.title")}</h1>
          <p className="tagline">{t("app.tagline")}</p>
        </div>
        <div className="lang-switch">
          {availableLangs().map((l) => (
            <button
              key={l.code}
              data-active={l.code === uiLang}
              onClick={() => switchLang(l.code)}
            >
              {l.label}
            </button>
          ))}
        </div>
      </header>

      {needsFfmpeg && (
        <div className="notice warn">
          <AlertTriangle size={20} />
          <div>
            <h2>{t("setup.ffmpeg.heading")}</h2>
            <p>{t("setup.ffmpeg.body")}</p>
            <code className="snippet">{t("setup.ffmpeg.command")}</code>
          </div>
        </div>
      )}

      {needsModel && status && (
        <div className="notice warn">
          <Download size={20} />
          <div style={{ flex: 1 }}>
            <h2>{t("setup.model.heading")}</h2>
            <p>{t("setup.model.body")}</p>
            {status.models.map((m) => (
              <div className="model-row" key={m.id}>
                <small>{t(`setup.model.${m.id}`)}</small>
                {m.installed ? (
                  <span>{t("setup.model.installed")}</span>
                ) : (
                  <button
                    className="btn"
                    disabled={downloading !== null}
                    onClick={() => download(m.id)}
                  >
                    {downloading === m.id ? (
                      <>
                        <Loader2 size={15} className="spin" />
                        {t("setup.model.downloading")} {Math.round(downloadPct * 100)}%
                      </>
                    ) : (
                      <>
                        <Download size={15} />
                        {t("setup.model.download")}
                      </>
                    )}
                  </button>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {!video && (
        <div className="dropzone" data-active={dragging}>
          <Clapperboard size={34} className="icon" />
          <h2>{dragging ? t("drop.active") : t("drop.headline")}</h2>
          <p>{t("drop.hint")}</p>
          <button className="btn primary" style={{ marginTop: 12 }} onClick={chooseVideo}>
            <FolderOpen size={16} />
            {t("drop.button")}
          </button>
        </div>
      )}

      {video && (
        <div className="card filebar">
          <div className="name">
            <Clapperboard size={18} color="var(--accent)" />
            <span>{basename(video)}</span>
          </div>
          <button className="btn ghost" onClick={chooseVideo} disabled={phase === "working"}>
            {t("file.change")}
          </button>
        </div>
      )}

      {video && phase !== "working" && (
        <>
          <section>
            <span className="section-title">{t("style.heading")}</span>
            <div className="styles">
              {styles.map((s) => (
                <button
                  key={s.id}
                  className="style-card"
                  data-selected={s.id === styleId}
                  onClick={() => setStyleId(s.id)}
                >
                  <StylePreview info={s} />
                  <div className="meta">
                    <strong>{t(`style.${s.id}`)}</strong>
                    <small>{t(`style.${s.id}.desc`)}</small>
                  </div>
                </button>
              ))}
            </div>
          </section>

          <section>
            <span className="section-title">{t("lang.heading")}</span>
            <div className="choices">
              {SPOKEN_LANGS.map((code) => (
                <button
                  key={code}
                  className="chip"
                  data-selected={code === spokenLang}
                  onClick={() => setSpokenLang(code)}
                >
                  {t(`lang.${code}`)}
                </button>
              ))}
            </div>
          </section>
        </>
      )}

      {video && phase !== "done" && (
        <button className="btn primary" disabled={!canStart} onClick={start}>
          <Wand2 size={16} />
          {t("action.start")}
        </button>
      )}

      {phase === "working" && (
        <div className="card progress">
          <div className="row">
            <span>{t(`stage.${progress.stage}`)}</span>
            <span>{Math.round(progress.percent * 100)}%</span>
          </div>
          <div className="track">
            <div style={{ width: `${Math.max(2, progress.percent * 100)}%` }} />
          </div>
        </div>
      )}

      {phase === "done" && output && (
        <div className="card result">
          <div className="name" style={{ display: "flex", gap: 10, alignItems: "center" }}>
            <CircleCheck size={20} color="var(--accent)" />
            <h2>{t("result.heading")}</h2>
          </div>
          <div className="path">{output}</div>
          <div className="actions">
            <button
              className="btn primary"
              onClick={() => invoke("reveal_in_finder", { path: output })}
            >
              <FolderOpen size={16} />
              {t("action.reveal")}
            </button>
            <button
              className="btn"
              onClick={() => {
                setVideo(null);
                setOutput(null);
                setPhase("idle");
              }}
            >
              {t("action.again")}
            </button>
          </div>
        </div>
      )}

      {phase === "error" && error && (
        <div className="notice danger">
          <AlertTriangle size={20} />
          <div>
            <h2>{t("error.heading")}</h2>
            <p>{t(`error.${error.code}`)}</p>
            {error.detail && (
              <details className="details">
                <summary>{t("action.details")}</summary>
                <pre>{error.detail}</pre>
              </details>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/// A mock video frame showing how the preset places and colours its captions.
function StylePreview({ info }: { info: StyleInfo }) {
  const words = t("style.preview.words").split(" ");
  const active = Math.min(1, words.length - 1);
  // ASS numpad alignment: 4 to 6 is vertically centred, everything else here
  // sits at the bottom.
  const align = info.alignment >= 4 && info.alignment <= 6 ? "middle" : "bottom";
  const shown = words.slice(0, info.max_words_per_line);

  return (
    <div className="frame" data-align={align}>
      <div>
        {shown.map((w, i) => (
          <span
            key={i}
            style={{
              color: i === active ? info.highlight_color : info.base_color,
              textTransform: info.uppercase ? "uppercase" : "none",
              marginRight: 5,
            }}
          >
            {w}
          </span>
        ))}
      </div>
    </div>
  );
}
