import { LaptopIcon, MoonIcon, SunIcon } from "lucide-react";
import { type Theme, useTheme } from "@/components/theme-provider";
import { Button } from "@/components/ui/button";

const next: Record<Theme, Theme> = { system: "light", light: "dark", dark: "system" };
const icons = { system: LaptopIcon, light: SunIcon, dark: MoonIcon };

export function ThemeToggle() {
  const { theme, setTheme } = useTheme();
  const Icon = icons[theme];
  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={() => setTheme(next[theme])}
      aria-label={`Theme: ${theme}. Switch to ${next[theme]}.`}
      title={`Theme: ${theme}`}
    >
      <Icon />
    </Button>
  );
}
