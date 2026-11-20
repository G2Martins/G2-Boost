import { PageBody, PageHeader } from "@/components/shell/page-header";
import { useI18n } from "@/lib/i18n";

// r11:activity: notification center + run history (agent A1 fills this page).
export function ActivityPage() {
  const { t } = useI18n();
  return (
    <>
      <PageHeader title={t("activity.title")} subtitle={t("activity.subtitle")} />
      <PageBody>{null}</PageBody>
    </>
  );
}
