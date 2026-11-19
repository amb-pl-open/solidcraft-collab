# User guide

For the people using a solidcraft-collab site. Each section names the feature it covers; when a
branch changes how a feature behaves, it updates the matching section.

## Accounts
*(F-C01)*

- **Getting an account:** on the sign-in page choose *Ask for an account*, enter your name, any
  email (a personal one is fine) and a password of at least 10 characters. An admin approves you
  into a team; until then you can't sign in.
- **Forgot your password:** ask an admin. They give you a one-time password; after signing in
  with it you must choose your own before anything else works.
- **Too many wrong passwords** pause sign-in for a few minutes.

## Teams and roles
*(F-C02)*

| Role | Can |
|---|---|
| Viewer | open projects shared with them, view designs, comment |
| Editor | everything a viewer can, plus create projects, upload, edit and save |
| Owner | everything an editor can, plus manage the team's members and free a part someone left locked |
| Admin (site-wide) | approve accounts, create teams, reset passwords, see every project |

Teams never see each other's projects. Only owners and admins see members' email addresses.

## Projects
*(F-C03)*

- **Joint project:** the whole team can open it.
- **Personal project:** only you, until you share it with named teammates (*Who can open this
  project* on the project page).

## Versions
*(F-C04, F-C07)*

Every save is a new version with your name and an optional note. *History & comments* on a file
lists its versions; **Restore** brings an older one back as a new version (nothing is lost).
Comments are kept per file.

## Working together
*(F-C05, F-C06)*

- Opening a design for editing takes its **edit lock**: you edit, others view. Different people
  can edit different parts of the same project at the same time.
- Coloured avatars show who is in the project, which file they have open, whether they are
  editing or viewing, and what they are working on.
- People viewing a part you edit see your saved versions appear live.
- Leaving or going idle for 3 minutes frees the part. A team owner can free it sooner (*Free it*);
  whoever held it keeps their unsaved changes on screen but must take the part again to save.

## Editing a design
*(F-C08)*

**Open** starts SolidCraft in the page. **Save version** stores it (with an optional note). The
bar shows *unsaved changes* until you save; leaving with unsaved changes asks first.

## Uploading CAD files
*(F-C09)*

*Upload files* adds files to the project. A CAD file (STEP, IGES, and on sites whose SolidCraft
reads them, SolidWorks, Fusion, Inventor, CATIA and others) also gets a SolidCraft design made
from it. The upload is saved at once; the design appears in the list when ready. If the server
is busy or the file can't be read, the file is still kept and the activity feed says so.
