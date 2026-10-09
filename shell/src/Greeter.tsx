// The login screen.
//
// Same binary, same stylesheet, same motifs as the desktop behind it — which
// is the whole argument for not reaching for a second UI toolkit here. It used
// to be a ratatui app on a bare VT, where the console's sixteen colours
// quantised the palette and the clock had to be built out of box characters.
// greetd runs it under `cage` now, so it is a real surface with the real face.
//
// greetd owns authentication. This collects a string and is told yes or no.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getTheme } from "./lib/ipc";
import { Wordmark } from "./components/Wordmark";

type Step =
  | { kind: "prompt"; message: string; secret: boolean }
  | { kind: "ready" }
  | { kind: "failed"; message: string };

const DEFAULT_USER = "";

export default function Greeter() {
  const [stage, setStage] = useState<"user" | "answer" | "starting">("user");
  const [prompt, setPrompt] = useState("LOGIN");
  const [secret, setSecret] = useState(false);
  const [value, setValue] = useState(DEFAULT_USER);
  const [user, setUser] = useState("");
  const [error, setError] = useState("");
  const [host, setHost] = useState("");
  const input = useRef<HTMLInputElement>(null);

  // The theme comes from /etc/khadi/theme.toml here: the greeter runs as the
  // `greeter` user, which cannot read anyone's home directory. khadi-core
  // already falls back to the system copy, so nothing special is needed —
  // except that `khadi-install --system` has to have put it there.
  useEffect(() => {
    getTheme()
      .then((t) => {
        const hex = (s: string) => {
          const n = parseInt(s.slice(1), 16);
          return `${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}`;
        };
        const r = document.documentElement.style;
        r.setProperty("--c", hex(t.text));
        r.setProperty("--ground", t.ground);
        r.setProperty("--grid", t.ramp[1]);
      })
      .catch(() => {});
    invoke<string>("greet_host").then(setHost).catch(() => {});
  }, []);

  useEffect(() => void input.current?.focus(), [stage, prompt]);

  const fail = useCallback((m: string) => {
    setError(m);
    setStage("user");
    setPrompt("LOGIN");
    setSecret(false);
    setValue("");
    void invoke("greet_cancel").catch(() => {});
  }, []);

  const apply = useCallback(
    async (s: Step) => {
      if (s.kind === "prompt") {
        setError("");
        setPrompt(s.message.replace(/:\s*$/, "").toUpperCase());
        setSecret(s.secret);
        setValue("");
        setStage("answer");
        return;
      }
      if (s.kind === "failed") {
        fail(s.message);
        return;
      }
      // Authenticated. Hand greetd the session and stop drawing.
      setStage("starting");
      const cmd = await invoke<string[]>("greet_session_cmd");
      const r = await invoke<Step>("greet_start", { cmd });
      if (r.kind === "failed") fail(r.message);
    },
    [fail],
  );

  const submit = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      if (stage === "starting") return;
      try {
        if (stage === "user") {
          setUser(value);
          apply(await invoke<Step>("greet_begin", { username: value }));
        } else {
          apply(await invoke<Step>("greet_answer", { answer: value }));
        }
      } catch (err) {
        fail(String(err));
      }
    },
    [stage, value, apply, fail],
  );

  return (
    <div className="relative flex h-full w-full flex-col justify-between">
      {/* The frame, as the lock screen has it: full-bleed, ticked at both ends. */}
      <header className="relative shrink-0 px-[calc(1.6vh*var(--ui-scale))] pt-[calc(1.4vh*var(--ui-scale))]">
        <div className="flex items-baseline justify-between text-[calc(1.2vh*var(--ui-scale))] tracking-[0.2vh]">
          <span>KHADI</span>
          <span className="opacity-60">{stage === "starting" ? "STARTING SESSION" : "LOGIN"}</span>
        </div>
        <Rule />
      </header>

      <main className="flex flex-1 flex-col items-center justify-center">
        {/* The logo is the hero. It replaced a clock — a login screen does
            not need to tell the time, and the clock was filling the space the
            mark should have had. */}
        <Wordmark className="h-[calc(11vh*var(--ui-scale))] w-auto text-[rgb(var(--c))]" />

        <form onSubmit={submit} className="mt-[calc(6vh*var(--ui-scale))] w-[42vh] max-w-[80vw]">
          <div className="flex items-baseline justify-between text-[calc(1.1vh*var(--ui-scale))] tracking-[0.2vh]">
            <span className="opacity-60">{prompt}</span>
            {user && stage !== "user" && <span className="opacity-40">{user}</span>}
          </div>
          <Rule />
          <input
            ref={input}
            type={secret ? "password" : "text"}
            value={value}
            disabled={stage === "starting"}
            onChange={(e) => setValue(e.target.value)}
            autoFocus
            spellCheck={false}
            autoComplete="off"
            className="mt-[calc(1vh*var(--ui-scale))] w-full border border-[rgba(var(--c),0.45)]
                       bg-[rgba(var(--c),0.06)] px-[calc(1.2vh*var(--ui-scale))]
                       py-[calc(0.9vh*var(--ui-scale))] text-center
                       text-[calc(1.7vh*var(--ui-scale))] tracking-[0.3vh]
                       text-[rgb(var(--c))] outline-none
                       focus:border-[rgb(var(--c))] focus:bg-[rgba(var(--c),0.12)]"
          />
          {/* Errors say what happened. They do not apologise and they are not
              vague: greetd's own description is more useful than a generic
              "login failed", and it is what the journal will say too. */}
          <p
            className="mt-[calc(1vh*var(--ui-scale))] h-[calc(2vh*var(--ui-scale))] text-center
                       text-[calc(1.2vh*var(--ui-scale))] tracking-[0.2vh]"
            style={{ opacity: error ? 1 : 0 }}
          >
            {error || " "}
          </p>
        </form>
      </main>

      <footer className="relative shrink-0 px-[calc(1.6vh*var(--ui-scale))] pb-[calc(1.4vh*var(--ui-scale))]">
        <Rule flip />
        <div className="flex items-baseline justify-between text-[calc(1.2vh*var(--ui-scale))] tracking-[0.2vh] opacity-55">
          <span>{host}</span>
          <span>{stage === "user" ? "ENTER USERNAME" : stage === "answer" ? "ENTER TO SUBMIT" : ""}</span>
        </div>
      </footer>
    </div>
  );
}

/** The motif: a hairline with a tick at each end. `flip` hangs the ticks
 *  upward, so a rule at the foot of the screen closes the frame instead of
 *  opening a second one. */
function Rule({ flip = false }: { flip?: boolean }) {
  return (
    <div className="relative my-[calc(0.5vh*var(--ui-scale))] h-px w-full bg-[rgba(var(--c),0.3)]">
      <span
        className={`absolute left-0 h-[calc(0.7vh*var(--ui-scale))] w-px bg-[rgba(var(--c),0.3)] ${flip ? "bottom-0" : "top-0"}`}
      />
      <span
        className={`absolute right-0 h-[calc(0.7vh*var(--ui-scale))] w-px bg-[rgba(var(--c),0.3)] ${flip ? "bottom-0" : "top-0"}`}
      />
    </div>
  );
}
