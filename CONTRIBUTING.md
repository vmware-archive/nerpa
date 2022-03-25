# Contributing to the Nerpa project
The Nerpa team welcomes contributions from the community. Before you start working with Nerpa, please read our [Developer Certificate of Origin](https://cla.vmware.com/dco). All contributions to this repository must be signed as described on this pace. Your signature certifies that you wrote the patch or have the right to pass it on as an open-source path.

Contributor guidelines help us stay on top of things. These contributing guidelines were heavily modeled after Puppet's [[Contributing Guidelines](https://github.com/puppetlabs/puppet/blob/main/CONTRIBUTING.md). Both DDlog and P4 are fast-moving technologies, available on increasing numbers of platforms. We want to keep it as easy as possible to contribute changes that make things work for you.

## Project management
We mostly use github tools for managing the project.  Bugs and
questions should be filed as [github
issues](https://github.com/vmware/nerpa/issues).
Improvements should be submitted as [pull
requests](https://github.com/vmware/nerpa/pulls).

When submitting bugs, please clearly describe it, including steps to reproduce any bugs. Please provide version and operating system information for major components (DDlog, P4, etc). Trivial changes do not need an issue -- feel free to make a pull request.

## Using git to contribute
* Make sure you have a Github account.
* Submit a GitHub issue if a relevant one does not exist.
* For the repository using the "fork" button on github using these [instructions](https://help.github.com/articles/fork-a-repo/). The main step is the following:

```shell
git remote add upstream https://github.com/vmware/nerpa.git
```

## Making changes
Here is a step-by-step guide to submitting contributions:
1. Create a new branch for each fix, with a descriptive name: `git checkout -b your_branch_name`
2. `git add <files that changed>`
3. `git commit -s -m "Description of commit"` Each logically independent change should have a separate commit with an informative message.
4. `git fetch upstream`
5. `git rebase upstream/master`
6. Resolve any conflicts. As you find and fix conflicts, `git add` the merged files. At the end, you may need to use `git rebase --continue` or `git rebase --skip`.
7. Test and analyze the merged version.
8. `git push -f origin your_branch_name`
9. Create a pull request to merge your new branch into `main`, using the Web UI.
10. Wait for approval; make any requested changes; and squash + merge one commit.
11. Delete your branch after merge: `git branch -D your_branch_name`

