#/bin/bash
ansible-playbook -u ubuntu --private-key ~/.ssh/rs3_london.pem -i rs3.common-work-education.co.uk, playbook_prod.yml