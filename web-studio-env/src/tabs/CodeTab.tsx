import type { Environment, EnvApp } from "../types";

interface Props {
  env: Environment;
  app: EnvApp | null;
  appSlug: string;
}

export function CodeTab({ env, app: _app, appSlug }: Props) {
  const isProd = env.type === "prod";
  const envSlug = env.slug || "dev";
  const iframeUrl = `https://code.${envSlug}.mynetwk.biz/?folder=/apps/${appSlug}`;

  return (
    <div className="flex flex-col h-full w-full">
      {isProd && (
        <div className="flex items-center gap-2 px-4 py-1.5 shrink-0 text-xs bg-err/10 text-err">
          🔒 Production — code view is read-only
        </div>
      )}
      <iframe
        src={iframeUrl}
        className="flex-1 w-full border-0 bg-[#1e1e1e]"
        title={`Code editor - ${appSlug}`}
        allow="clipboard-read; clipboard-write"
      />
    </div>
  );
}
