import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Layout from '@theme/Layout';
import clsx from 'clsx';
import styles from './index.module.css';

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  const logoUrl = useBaseUrl('/img/wax-full-logo.png');

  return (
    <header className={styles.hero}>
      <div className={clsx('container', styles.heroContent)}>
        <img
          className={styles.logo}
          src={logoUrl}
          alt="Wax: design system coverage analytics"
        />
        <p className={styles.tagline}>{siteConfig.tagline}</p>
        <div className={styles.actions}>
          <Link
            className="button button--primary button--lg"
            to="/docs/getting-started/">
            Get started
          </Link>
          <Link
            className="button button--outline button--secondary button--lg"
            href="https://github.com/Daio-io/wax">
            GitHub
          </Link>
        </div>
      </div>
    </header>
  );
}

export default function Home() {
  return (
    <Layout
      title="Design system coverage analytics"
      description="Design system coverage analytics for agents">
      <main>
        <HomepageHeader />
      </main>
    </Layout>
  );
}
