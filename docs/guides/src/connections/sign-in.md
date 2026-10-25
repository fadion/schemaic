# Signing in and TLS

Most connections sign in with a user and a password. Where an engine offers another way, the
connection form shows an **Authentication** picker above **User**.

## Sign-in methods

| Method                            | Engines                     | What it needs                                    |
|-----------------------------------|-----------------------------|--------------------------------------------------|
| **User and password**             | All but SQLite              | **User** and **Password**                        |
| **Windows (this account)**        | SQL Server, on Windows only | Nothing — the account Schemaic runs as           |
| **Microsoft Entra (Azure CLI)**   | SQL Server                  | The Azure CLI, signed in with `az login`         |
| **AWS IAM (AWS CLI)**             | MySQL, MariaDB, PostgreSQL  | **User**, and the AWS CLI signed in              |
| **Google Cloud IAM (gcloud)**     | MySQL, MariaDB, PostgreSQL  | **User**, and the Google Cloud CLI signed in     |

- *Windows* signs in as the Windows account Schemaic is running under. The server must know that
  account as a Windows login. The option is only there in the Windows build.
- *Microsoft Entra* signs in as whoever is signed in to the Azure CLI. Run `az login` first. Use it
  for Azure SQL Database and other servers that accept Entra accounts.
- *AWS IAM* and *Google Cloud IAM* are for Amazon RDS and Cloud SQL. They sign in as the **User**
  you give, with a token from the cloud's own command-line tool — run `aws configure` or
  `aws sso login`, or `gcloud auth login`, first. On RDS the user needs the `rds_iam` role
  (PostgreSQL) or the `AWSAuthenticationPlugin` (MySQL).

With the last three, Schemaic asks the CLI for a token each time it needs one and never saves it.
If the CLI isn't signed in, the connection fails with a message saying which command to run.

An Azure SQL serverless database that has paused takes about a minute to wake. The first
connection wakes it but may time out; connect again shortly.

## Encrypting the connection

**SSL / TLS**, in the form's **Network** section, decides whether the connection is encrypted, and
how much Schemaic checks the server's certificate. Each step up adds to the one before it.

| Mode            | What it does                                                         |
|-----------------|----------------------------------------------------------------------|
| **Disable**     | Connects in plain text. The default for a new connection.            |
| **Prefer**      | Encrypts if the server offers it. Doesn't check the certificate.     |
| **Require**     | Refuses to connect without encryption. Doesn't check the certificate.|
| **Verify CA**   | Encrypts, and checks the certificate against a trusted authority.    |
| **Verify full** | Also checks that the certificate is for the host you connect to.     |

Only **Verify CA** and **Verify full** protect you from someone in the middle; the first three keep
out eavesdroppers at best. On MySQL and MariaDB, **Verify CA** checks the host name too, so it
behaves as **Verify full**.

The certificate fields appear once you pick a mode that encrypts:

- **CA certificate**, with the two verifying modes — leave it empty to trust the certificate authorities your operating system
  trusts, including any your organisation has installed. Name a file to trust only that one; for
  RDS or Cloud SQL, this is where the provider's CA bundle goes.
- **Client certificate** and **Client key** — only for servers that ask you to prove who you are
  (mutual TLS). Both must be PEM files, and the key can't be passphrase-protected.

Token sign-in sets a floor on encryption, whatever mode you picked: Microsoft Entra always connects
at **Verify full**, AWS IAM at least at **Require**, and Google Cloud IAM at least at **Verify CA**.
The form tells you when this happens.
