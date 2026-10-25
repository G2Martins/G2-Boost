import { PerfisTab } from "@/components/profiles/perfis-tab";
import { RestoreTab } from "@/components/profiles/restore-tab";
import { PageBody, PageHeader } from "@/components/shell/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/primitives";
import { useBoostStates, useElevated } from "@/lib/boost-hooks";
import { boot } from "@/lib/boot";
import { useI18n } from "@/lib/i18n";

export function ProfilesPage() {
  const { t } = useI18n();
  const elevated = useElevated();
  const { states, refresh } = useBoostStates();
  return (
    <PageHeader
      title={t("nav.profiles")}
      subtitle={t("profiles.subtitle")}
      tabs={
        <Tabs
          defaultValue={boot.startTab === "restore" ? "restore" : "profiles"}
          onValueChange={() => void refresh()}
        >
          <TabsList>
            <TabsTrigger value="profiles">{t("profiles.tab.profiles")}</TabsTrigger>
            <TabsTrigger value="restore">{t("profiles.tab.restore")}</TabsTrigger>
          </TabsList>
          <TabsContent value="profiles">
            <PageBody className="px-0 pb-0">
              <PerfisTab states={states} refresh={refresh} elevated={elevated} />
            </PageBody>
          </TabsContent>
          <TabsContent value="restore">
            <PageBody className="px-0 pb-0">
              <RestoreTab elevated={elevated} />
            </PageBody>
          </TabsContent>
        </Tabs>
      }
    />
  );
}
