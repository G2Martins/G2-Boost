import { createContext, type ReactNode, useCallback, useContext, useMemo, useState } from "react";
import { en } from "@/locales/en";
import { ptBR } from "@/locales/pt-BR";

export const LOCALES = { "pt-BR": ptBR, en } as const;
export type Locale = keyof typeof LOCALES;
export type I18nKey = keyof typeof ptBR;
type Params = Record<string, string | number>;

const STORAGE_KEY = "g2boost.locale";
const DEFAULT_LOCALE: Locale = "pt-BR";

function readStoredLocale(): Locale {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v && v in LOCALES) return v as Locale;
  } catch {
    /* storage unavailable: fall through */
  }
  return DEFAULT_LOCALE;
}

type Ctx = {
  locale: Locale;
  setLocale: (l: Locale) => void;
  t: (key: I18nKey, params?: Params) => string;
};

const I18nContext = createContext<Ctx | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(readStoredLocale);
  const setLocale = useCallback((l: Locale) => {
    setLocaleState(l);
    try {
      localStorage.setItem(STORAGE_KEY, l);
    } catch {
      /* ignore */
    }
  }, []);
  const t = useCallback(
    (key: I18nKey, params?: Params) => {
      const raw: string = LOCALES[locale][key] ?? ptBR[key] ?? key;
      if (!params) return raw;
      return raw.replace(/\{(\w+)\}/g, (_, k: string) => String(params[k] ?? `{${k}}`));
    },
    [locale],
  );
  const value = useMemo(() => ({ locale, setLocale, t }), [locale, setLocale, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): Ctx {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useI18n must be used inside <I18nProvider>");
  return ctx;
}
