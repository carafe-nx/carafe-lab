import type { ReactNode } from "react";
import { PageHeader } from "./PageHeader";
import styles from "./Page.module.css";

type PageProps = {
  title: string;
  aside?: ReactNode;
  children: ReactNode;
};

export function Page({ title, aside, children }: PageProps) {
  return (
    <>
      <PageHeader title={title} aside={aside} />
      <div className={styles.content}>{children}</div>
    </>
  );
}
