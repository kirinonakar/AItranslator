import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const DEFAULT_BASE_URL = "http://localhost:1234/v1";

const modelsEndpoint = (baseUrl: string) => {
  const trimmed = baseUrl.trim().replace(/\/+$/, "");
  return trimmed.endsWith("/models") ? trimmed : `${trimmed}/models`;
};

const chatEndpoint = (baseUrl: string) => {
  const trimmed = baseUrl.trim().replace(/\/+$/, "");
  return trimmed.endsWith("/chat/completions") ? trimmed : `${trimmed}/chat/completions`;
};

const bearerAuthorization = (apiKey?: string) => {
  const value = (apiKey ?? "lm-studio").trim() || "lm-studio";
  return /^Bearer\s+/i.test(value) ? value : `Bearer ${value}`;
};

const optionalBearerAuthorization = (apiKey?: string) => {
  const value = apiKey?.trim();
  if (!value) return undefined;
  return /^Bearer\s+/i.test(value) ? value : `Bearer ${value}`;
};

export default defineConfig({
  plugins: [
    react(),
    {
      name: "ai-provider-proxies",
      configureServer(server) {
        const handleModelsRequest = async (req: import("http").IncomingMessage, res: import("http").ServerResponse) => {
          try {
            const requestUrl = new URL(req.url ?? "", "http://localhost");
            const baseUrl = requestUrl.searchParams.get("baseUrl") ?? DEFAULT_BASE_URL;
            const apiKey = requestUrl.searchParams.get("apiKey") ?? undefined;

            if (!/^https?:\/\//i.test(baseUrl)) {
              res.statusCode = 400;
              res.setHeader("content-type", "application/json");
              res.end(JSON.stringify({ error: "Only http and https provider URLs are supported." }));
              return;
            }

            const authorization = optionalBearerAuthorization(apiKey);
            const upstream = await fetch(modelsEndpoint(baseUrl), {
              headers: {
                accept: "application/json",
                ...(authorization ? { authorization } : {}),
              },
            });
            const body = await upstream.text();

            res.statusCode = upstream.status;
            res.setHeader("content-type", upstream.headers.get("content-type") ?? "application/json");
            res.end(body);
          } catch (error) {
            res.statusCode = 502;
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify({ error: String(error) }));
          }
        };

        server.middlewares.use("/__provider_models", handleModelsRequest);
        server.middlewares.use("/__lmstudio_models", handleModelsRequest);

        server.middlewares.use("/__chat_completions", async (req, res) => {
          if (req.method !== "POST") {
            res.statusCode = 405;
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify({ error: "Method not allowed" }));
            return;
          }

          try {
            const chunks: Buffer[] = [];
            for await (const chunk of req) {
              chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
            }

            const proxyRequest = JSON.parse(Buffer.concat(chunks).toString("utf8")) as {
              baseUrl?: string;
              apiKey?: string;
              body?: unknown;
            };
            const baseUrl = proxyRequest.baseUrl ?? DEFAULT_BASE_URL;

            if (!/^https?:\/\//i.test(baseUrl)) {
              res.statusCode = 400;
              res.setHeader("content-type", "application/json");
              res.end(JSON.stringify({ error: "Only http and https API URLs are supported." }));
              return;
            }

            const upstream = await fetch(chatEndpoint(baseUrl), {
              method: "POST",
              headers: {
                accept: "text/event-stream, application/json",
                authorization: bearerAuthorization(proxyRequest.apiKey),
                "content-type": "application/json",
              },
              body: JSON.stringify(proxyRequest.body ?? {}),
            });

            res.statusCode = upstream.status;
            res.setHeader("content-type", upstream.headers.get("content-type") ?? "application/json");
            res.setHeader("cache-control", "no-cache, no-transform");
            res.setHeader("connection", "keep-alive");
            res.setHeader("x-accel-buffering", "no");

            if (!upstream.body) {
              res.end();
              return;
            }

            res.flushHeaders?.();
            const reader = upstream.body.getReader();
            while (true) {
              const { done, value } = await reader.read();
              if (done) break;
              res.write(Buffer.from(value));
            }
            res.end();
          } catch (error) {
            res.statusCode = 502;
            res.setHeader("content-type", "application/json");
            res.end(JSON.stringify({ error: String(error) }));
          }
        });
      },
    },
  ],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
