import type { Language } from "./types";

const languageCodes: Record<string, string> = {
  English: "en",
  Korean: "ko",
  Japanese: "ja",
  "Chinese (Simplified)": "zh-Hans",
  "Chinese (Traditional)": "zh-Hant",
  Spanish: "es",
  French: "fr",
  German: "de",
  Russian: "ru",
};

const alphabetic = /\p{Alphabetic}/u;

export const resolveSourceLanguage = (
  sourceLang: Language,
  text: string,
  documentSourceLang: Language = "Auto Detect",
): Language => {
  if (sourceLang !== "Auto Detect") return sourceLang;

  let kana = 0;
  let han = 0;
  let otherLetters = 0;

  for (const character of text) {
    const code = character.codePointAt(0)!;
    if (
      (code >= 0x3041 && code <= 0x3096) ||
      (code >= 0x309d && code <= 0x309f) ||
      (code >= 0x30a1 && code <= 0x30fa) ||
      (code >= 0x30fd && code <= 0x30ff) ||
      (code >= 0x31f0 && code <= 0x31ff) ||
      (code >= 0xff66 && code <= 0xff6f) ||
      (code >= 0xff71 && code <= 0xff9d) ||
      (code >= 0x1b000 && code <= 0x1b122)
    ) {
      kana++;
    } else if (
      (code >= 0x3400 && code <= 0x4dbf) ||
      (code >= 0x4e00 && code <= 0x9fff) ||
      (code >= 0xf900 && code <= 0xfaff) ||
      (code >= 0x20000 && code <= 0x323af)
    ) {
      han++;
    } else if (
      (code >= 0x3099 && code <= 0x309c) || code === 0x30fc ||
      code === 0xff70 || code === 0xff9e || code === 0xff9f
    ) {
      // Voicing and vowel-length marks support kana but are not language evidence alone.
    } else if (alphabetic.test(character)) {
      otherLetters++;
    }
  }

  // Kana distinguishes Japanese from Han-only text. Ignore punctuation and
  // avoid overriding another language just because it quotes a Japanese word.
  if (kana > 0 && kana + han >= otherLetters) return "Japanese";

  // A Japanese document can contain chunks made only of kanji or numbers.
  // Keep its context, but let the model detect passages in other languages.
  if (documentSourceLang === "Japanese" && kana + han >= otherLetters) return "Japanese";
  return "Auto Detect";
};

export const createFormattedPrompt = (
  sourceLang: Language,
  targetLang: Language,
  text: string,
  documentSourceLang: Language = "Auto Detect",
) => {
  const resolvedSource = resolveSourceLanguage(sourceLang, text, documentSourceLang);
  const sourceCode = languageCodes[resolvedSource] ?? "auto";
  const targetCode = languageCodes[targetLang] ?? "en";
  const introduction = resolvedSource === "Auto Detect"
    ? `You are a professional translator. Identify the source language from the text itself and translate it into ${targetLang} (${targetCode}). Do not infer the source language from the requested target language.`
    : `You are a professional ${resolvedSource} (${sourceCode}) to ${targetLang} (${targetCode}) translator.`;
  const japaneseInstruction = resolvedSource === "Japanese" && targetLang !== "Japanese"
    ? `\nThe source text is Japanese. Read kanji together with hiragana and katakana as Japanese, including short fragments. Translate the Japanese prose into ${targetLang}; do not return the Japanese original as the translation.`
    : "";

  return introduction + japaneseInstruction +
    `\nAccurately convey the meaning and nuances of the original text using ${targetLang} grammar and vocabulary. Preserve paragraph breaks, formatting, and proper names where appropriate.` +
    `\nWhen the source and target languages differ, translate the prose rather than copying the source text unchanged.` +
    `\nProduce only the ${targetLang} translation, without language detection labels, explanations, or commentary.\n\nText to translate:\n${text}`;
};
