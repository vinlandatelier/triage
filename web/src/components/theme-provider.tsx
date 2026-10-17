import { createContext, useContext, useEffect, useState } from "react";

export type Theme = "dark" | "light" | "system";

// index.html reads the same key before paint; change both together.
export const THEME_STORAGE_KEY = "triage-theme";

type ThemeProviderProps = {
  children: React.ReactNode;
  defaultTheme?: Theme;
  storageKey?: string;
};

type ThemeProviderState = {
  theme: Theme;
  setTheme: (theme: Theme) => void;
};

const initialState: ThemeProviderState = {
  theme: "system",
  setTheme: () => null,
};

const ThemeProviderContext = createContext<ThemeProviderState>(initialState);

const isTheme = (value: unknown): value is Theme =>
  value === "dark" || value === "light" || value === "system";

// Storage throws when disabled (Safari private mode, blocked cookies); the theme then lasts one page load.
function readTheme(key: string): Theme | undefined {
  try {
    const stored = localStorage.getItem(key);
    return isTheme(stored) ? stored : undefined;
  } catch {
    return undefined;
  }
}

function writeTheme(key: string, theme: Theme) {
  try {
    localStorage.setItem(key, theme);
  } catch {}
}

export function ThemeProvider({
  children,
  defaultTheme = "system",
  storageKey = THEME_STORAGE_KEY,
  ...props
}: ThemeProviderProps) {
  const [theme, setTheme] = useState<Theme>(() => readTheme(storageKey) ?? defaultTheme);

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

  const value = {
    theme,
    setTheme: (theme: Theme) => {
      writeTheme(storageKey, theme);
      setTheme(theme);
    },
  };

  return (
    <ThemeProviderContext.Provider {...props} value={value}>
      {children}
    </ThemeProviderContext.Provider>
  );
}

export const useTheme = () => {
  const context = useContext(ThemeProviderContext);

  if (context === undefined) throw new Error("useTheme must be used within a ThemeProvider");

  return context;
};
