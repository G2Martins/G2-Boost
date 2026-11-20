import { PageBody, PageHeader } from "@/components/shell/page-header";
import { useI18n } from "@/lib/i18n";

// r11:luli: AI providers, Luli skills, Claude and the local MCP map (agent A4 fills this page).
export function ConnectionsPage() {
  const { t } = useI18n();
  return (
    <>
      <PageHeader title={t("connections.title")} subtitle={t("connections.subtitle")} />
      <PageBody>{null}</PageBody>
    </>
  );
}
