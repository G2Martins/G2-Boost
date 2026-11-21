import { useEffect, useState } from "react";
import { ClaudeConnectCard } from "@/components/claude-connect";
import { McpMapCard } from "@/components/connections/mcp-map";
import { ProvidersTab } from "@/components/connections/providers";
import { SkillsTab } from "@/components/connections/skills";
import { LuliCard } from "@/components/luli-card";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/primitives";
import { backend, inTauri } from "@/lib/backend";
import { takeStartTab } from "@/lib/boot";
import { useI18n } from "@/lib/i18n";

const TABS = ["providers", "skills", "mcp"] as const;
type Tab = (typeof TABS)[number];

// r11:luli: AI providers (key, live status, model), Luli skills, and MCP in both directions
// (G2 Boost into the AI apps; the read-only map of what those apps start).
export function ConnectionsPage() {
  const { t } = useI18n();
  const [tab, setTab] = useState<Tab>(() => {
    const arrival = takeStartTab() ?? "";
    return (TABS as readonly string[]).includes(arrival) ? (arrival as Tab) : "providers";
  });
  // A SKILL.md or .zip dropped on the window lands here (the shell routes it): take its bytes once.
  const [dropped, setDropped] = useState<ArrayBuffer | null>(null);
  useEffect(() => {
    if (!inTauri) return;
    const take = () =>
      backend
        .skillsTakeDrop()
        .then((b) => {
          setTab("skills");
          setDropped(b);
        })
        .catch(() => undefined); // nothing pending
    void take();
    const off = backend.on<null>("skills:dropped", () => void take());
    return () => {
      void off.then((f) => f());
    };
  }, []);

  return (
    <PageHeader
      title={t("connections.title")}
      subtitle={t("connections.subtitle")}
      tabs={
        <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)}>
          <TabsList>
            {TABS.map((k) => (
              <TabsTrigger key={k} value={k}>
                {t(`conn.tab.${k}` as const)}
              </TabsTrigger>
            ))}
          </TabsList>
          <TabsContent value="providers">
            <PageBody className="px-0 pb-0">
              <ProvidersTab />
            </PageBody>
          </TabsContent>
          <TabsContent value="skills">
            <PageBody className="px-0 pb-0">
              <SkillsTab dropped={dropped} onDropHandled={() => setDropped(null)} />
            </PageBody>
          </TabsContent>
          <TabsContent value="mcp">
            <PageBody className="px-0 pb-0">
              <LuliCard
                asks={[{ label: "conn.luli.ask.mcp" }, { label: "conn.luli.ask.connect" }]}
              />
              <ClaudeConnectCard style={rise(1)} />
              <McpMapCard style={rise(2)} />
            </PageBody>
          </TabsContent>
        </Tabs>
      }
    />
  );
}
