import { Hammer } from "lucide-react";
import { PageBody, PageHeader } from "@/components/shell/page-header";
import { EmptyState } from "@/components/ui/primitives";
import { type I18nKey, useI18n } from "@/lib/i18n";

// ponytail: scaffolding for the round-9 routes; each page replaces it, then this file goes.
export function PlaceholderPage({ title }: { title: I18nKey }) {
  const { t } = useI18n();
  return (
    <PageHeader
      title={t(title)}
      tabs={
        <PageBody className="px-0 pb-0">
          <EmptyState icon={<Hammer className="size-5" />} title={t("nav.soon")} />
        </PageBody>
      }
    />
  );
}
