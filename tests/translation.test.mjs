import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createFormattedPrompt, resolveSourceLanguage } from "../src/translation.ts";

const cases = JSON.parse(readFileSync(new URL("./fixtures/source-language.json", import.meta.url), "utf8"));

for (const entry of cases) {
  test(entry.name, () => {
    const source = entry.source ?? "Auto Detect";
    const document = entry.document ?? "Auto Detect";
    const target = entry.target ?? "Korean";
    assert.equal(resolveSourceLanguage(source, entry.text, document), entry.expected);

    const prompt = createFormattedPrompt(source, target, entry.text, document);
    assert.ok(prompt.endsWith(`Text to translate:\n${entry.text}`), "source is preserved verbatim");
    assert.ok(prompt.includes(`Produce only the ${target} translation`));
    if (entry.expected === "Auto Detect") {
      assert.ok(prompt.includes("Identify the source language from the text itself"));
    } else {
      assert.ok(prompt.includes(`professional ${entry.expected} (`));
    }
    assert.equal(prompt.includes("The source text is Japanese."), entry.expected === "Japanese" && target !== "Japanese");
  });
}

test("document context survives chunk boundaries and resuming from kanji-only text", () => {
  const text = "これは日本語の文章です。\n\n東京都\n\n第十二章";
  const document = resolveSourceLanguage("Auto Detect", text);
  assert.equal(document, "Japanese");
  for (const chunk of text.split("\n\n").slice(1)) {
    assert.equal(resolveSourceLanguage("Auto Detect", chunk, document), "Japanese");
    assert.ok(createFormattedPrompt("Auto Detect", "Korean", chunk, document).includes("Japanese (ja) to Korean (ko)"));
  }
});
