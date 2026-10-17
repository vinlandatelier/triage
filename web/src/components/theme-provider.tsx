import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";

export type Theme = "dark" | "light" | "system";

// index.html reads the same key and falls back to the OS before paint; change both together.
const THEME_STORAGE_KEY = "triage-theme";

type ThemeProviderState = {
  theme: Theme;
  setTheme: (theme: Theme) => void;
};

const ThemeProviderContext = createContext<ThemeProviderState | undefined>(undefined);

const isTheme = (value: unknown): value is Theme =>
  value === "dark" || value === "light" || value === "system";

// Storage throws when disabled (Safari private mode, blocked cookies); the theme then lasts one page load.
function readTheme(): Theme | undefined {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    return isTheme(stored) ? stored : undefined;
  } catch {
    return undefined;
  }
}

function writeTheme(theme: Theme) {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {}
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(() => readTheme() ?? "system");

  useEffect(() => {
    const root = window.document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      root.classList.remove("light", "dark");
      root.classList.add(theme === "system" ? (media.matches ? "dark" : "light") : theme);
    };
    apply();
    if (theme !== "system") return;
    // The upstream provider reads the OS preference once; this keeps `system` live.
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);

  const setTheme = useCallback((next: Theme) => {
    writeTheme(next);
    setThemeState(next);
  }, []);
  const value = useMemo(() => ({ theme, setTheme }), [theme, setTheme]);

  return <ThemeProviderContext.Provider value={value}>{children}</ThemeProviderContext.Provider>;
}

export const useTheme = () => {
  const context = useContext(ThemeProviderContext);

  if (context === undefined) throw new Error("useTheme must be used within a ThemeProvider");

  return context;
};
